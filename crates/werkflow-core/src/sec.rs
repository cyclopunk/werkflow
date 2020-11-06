use acme2_slim::{cert::SignedCertificate, Account};
use cloudflare::{endpoints::{dns::{self, DeleteDnsRecord, ListDnsRecordsParams}, zone::{ListZones, ListZonesParams}}, framework::{async_api::ApiClient, Environment, async_api::Client, HttpApiClientConfig}};
use cloudflare::framework::auth::Credentials;
use anyhow::{anyhow, Result};
use dns::ListDnsRecords;

pub struct DnsProvider {
    client: Client
}

#[derive(Clone,Debug)]
pub enum Zone {
    ById(String),
    ByName(String)
}

#[derive(Clone,Debug)]
pub enum ZoneRecord {
    A(String, String),
    ProxiedA(String, String),
    TXT(String, String),
    // already existing records
    Id(String),
    Name(String)
}

/// Implementation of a DNS provider using Cloudflare 
impl DnsProvider {
    pub fn new(api_token: &str) -> Result<DnsProvider>{
        let creds = Credentials::UserAuthToken {
            token: api_token.to_string(),
        };

        let api_client = Client::new(
            creds,
            HttpApiClientConfig::default(),
            Environment::Production,
        ).map_err(|err| anyhow!("Could not create Cloudflare API client: {}", err))?;
        
        

        Ok(DnsProvider {
            client: api_client
        })
    }
    async fn get_id(&self, zone: &Zone) -> Result<String> {
        match zone {
            Zone::ById(id) => {
                Ok(id.clone())
            }
            Zone::ByName(name) => {
                let api_result  = self.client.request(&ListZones {
                    params: ListZonesParams {
                        name: Some(name.clone()),
                        ..Default::default()
                    }
                }).await?;

                match api_result.result.first() {
                    Some(zone) => Ok(zone.id.clone()),
                    None => Err(anyhow!("Error getting zone by name {}", name))
                }
            }
        }
    }

    async fn get_record_id(&self, zone: &Zone, record : &ZoneRecord) -> Result<String> {

        let name = match record {
            ZoneRecord::Id(id) => { return Ok(id.clone()) }            
            ZoneRecord::A(name, _) => { Some(name.clone()) }       
            ZoneRecord::ProxiedA(name, _) => { Some(name.clone()) }
            ZoneRecord::TXT(name, _) => { Some(name.clone()) }
            ZoneRecord::Name(name) => {Some(name.clone()) }
        };


        let api_result  = self.client.request(&ListDnsRecords {
            zone_identifier: &self.get_id(zone).await?,
            params: ListDnsRecordsParams {
                name: name,
                ..Default::default()
            }
        }).await?;

        match api_result.result.first() {
            Some(record) => Ok(record.id.clone()),
            None => Err(anyhow!("Error getting zone record."))
        }
    }

    pub async fn delete_record(&self, zone: &Zone, record : &ZoneRecord) -> Result<String> {
        let id = self.get_record_id(zone, record).await?;

        let api_result  = self.client.request(&DeleteDnsRecord {
            zone_identifier: &self.get_id(zone).await?,
            identifier: &id
        }).await?;

        Ok("Record deleted".into())
    }

    pub async fn add_or_replace(&self, zone: &Zone, record: &ZoneRecord) -> Result<String> {
        println!("Adding {:?} to {:?}", zone, record);
        let looked_up_zone = self.get_id(zone).await?;
        
        if let Ok(record_id) = self.get_record_id(zone, record).await {
            self.delete_record(zone, record).await?;
        }

        let params = match record {
            ZoneRecord::A(host, ip) => dns::CreateDnsRecordParams {
                name: &host,
                content: dns::DnsContent::A {
                    content: ip.parse()?
                },
                priority: None,
                proxied: None,
                ttl: None,
            },
            ZoneRecord::ProxiedA(host, ip) => dns::CreateDnsRecordParams {
                name: &host,
                content: dns::DnsContent::A {
                    content: ip.parse()?
                },
                priority: None,
                proxied: Some(true),
                ttl: None,
            },
            ZoneRecord::TXT(name, txt) => dns::CreateDnsRecordParams {
                name: &name,
                content: dns::DnsContent::TXT {
                    content: txt.into(),
                },
                priority: None,
                proxied: None,
                ttl: None,
            },
            _ => return Err(anyhow!("Invalid ZoneRecord type passed to add_or_replace."))
        };


        let response = self.client.request(&dns::CreateDnsRecord {
            zone_identifier: &looked_up_zone,
            params: params,
        }).await
        .map_err(|err| anyhow!("Could not create TXT record client: {}", err))?;
        

        Ok(response.result.id)
    }

}

struct CertificateProvider {
    account: Account,
    dns_provider: Option<DnsProvider>
}

/// Certificate provider that uses ACME2 to create
/// certificates with a dns challenge.
/// TODO Add http challenge and integrate with the web feature.CertificateProvider

impl CertificateProvider {
    async fn order_with_dns(&mut self, provider: DnsProvider,  domains: Vec<String>) -> Result<Vec<SignedCertificate>> {
        let order = self.account.create_order(&domains)
            .map_err(|err| anyhow!("Error creating LetsEncrypt Order {}", err))?;
        let mut certificates: Vec<SignedCertificate> = Vec::new();
        for challenge in order.get_dns_challenges() {
            let signature = challenge.signature()
                .map_err(|err| anyhow!("Could not get signature. {}", err))?;

            let domain_name = &challenge
                .domain()
                .expect("Domain name not in challenge");
            
            provider
                .add_or_replace(
                    &Zone::ByName(domain_name.into()), 
                    &ZoneRecord::TXT(format!("_acme-challenge.{}", domain_name).into(), signature)
                )
                .await?;

            challenge.validate(&self.account)
                .map_err(|err| anyhow!("Validation failed {}", err))?;

            let signer = self.account.certificate_signer();

            let cert = signer.sign_certificate(&order).unwrap();

            certificates.push(cert);
            
        }

        Ok(certificates)
    }
    async fn new(email : &str) -> Result<CertificateProvider> {
        use acme2_slim::Directory;

        let directory = Directory::lets_encrypt()
            .map_err(|err| anyhow!("Error creating LetsEncrypt directory: {}", err))?;
        
        let account = directory.account_registration()
                               .email(email)
                               .register()
                               .map_err(|err| anyhow!("Error registering LetsEncrypt directory: {}", err))?;

        

        Ok(CertificateProvider {
            account:account,
            dns_provider: None
        })
    }
}

#[cfg(test)]
mod test {
    use config::File;
    use config::Config;
    use anyhow::{Result, anyhow};
    use super::{CertificateProvider, DnsProvider, Zone, ZoneRecord};

    #[tokio::test(threaded_scheduler)]
    async fn test() -> Result<()> {      
        let mut config = Config::new();
        
        config
            .merge(File::with_name("config/security.toml"))?;  
        
        let dns = config.get_table("dns")?;

        if let Some(token) = dns.get("api_token"){
            let val = token.clone();
            
            let mut p = CertificateProvider::new("discourse@gmail.com").await?;

            let domains = vec!["test2.autobuild.cloud".into()];

            let certs = p.order_with_dns(
                DnsProvider::new(&val.into_str()?)?, 
                domains.clone()
            )
            .await?;

            for (i, cert) in certs.iter().enumerate() {
                cert.save_signed_certificate(format!("config/{}.cer", &domains[i]))
                    .map_err(|err| anyhow!("Could not save certificate: {}", err))?;
                cert.save_private_key(format!("config/{}.key", &domains[i]))
                    .map_err(|err| anyhow!("Could not save private_key: {}", err))?;
            }

            Ok(())
        } else { Err(anyhow!("Couldn't find api_key in config")) }

    }
}