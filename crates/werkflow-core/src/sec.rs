use acme2_slim::Account;
use cloudflare::{endpoints::{dns::{self, DeleteDnsRecord, ListDnsRecordsParams}, zone::{ListZones, ListZonesParams}}, framework::{async_api::ApiClient, Environment, async_api::Client, HttpApiClientConfig}};
use cloudflare::framework::auth::Credentials;
use anyhow::{anyhow, Result};
use dns::ListDnsRecords;

struct DnsProvider {
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
    TXT(String, String),
    // already existing records
    Id(String),
    Name(String)
}

impl DnsProvider {
    fn new(api_token: &str) -> Result<DnsProvider>{
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
    async fn delete_record(&self, zone: &Zone, record : &ZoneRecord) -> Result<String> {
        let id = self.get_record_id(zone, record).await?;

        let api_result  = self.client.request(&DeleteDnsRecord {
            zone_identifier: &self.get_id(zone).await?,
            identifier: &id
        }).await?;

        Ok("Record deleted".into())
    }

    async fn add_or_replace(&self, zone: &Zone, record: &ZoneRecord) -> Result<String> {
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
    account: Account
}

impl CertificateProvider {
    async fn new(provider: DnsProvider, email : &str, domain_name: &str) -> Result<CertificateProvider> {
        use acme2_slim::Directory;

        let domains = [domain_name];

        let directory = Directory::lets_encrypt().map_err(|err| anyhow!("Error creating LetsEncrypt directory: {}", err))?;
        
        let mut account = directory.account_registration()
                               .email(email)
                               .register()
                               .map_err(|err| anyhow!("Error registering LetsEncrypt directory: {}", err))?;

        let order = account.create_order(domain_name)
            .map_err(|err| anyhow!("Error creating LetsEncrypt Order {}", err))?;

        for challenge in order.challenges.clone() {
            if challenge.ctype() == "dns-01" {
                let signature = challenge.signature()
                    .map_err(|err| anyhow!("Could not get signature. {}", err))?;

                provider.add_or_replace(&Zone::ByName(domain_name.into()), &ZoneRecord::TXT(format!("_acme-challenge.{}", domain_name).into(), signature)).await?;

                challenge.validate(&account)
                    .map_err(|err| anyhow!("Validation failed {}", err))?;

                let signer = account.certificate_signer(&domains);

                let cert = signer.sign_certificate(&order).unwrap();
                cert.save_signed_certificate(format!("certs/{}.pem", domain_name))
                    .map_err(|err| anyhow!("Could not save signed certificate. {}", err))?;
                cert.save_private_key(format!("certs/{}.key", domain_name))
                    .map_err(|err| anyhow!("Could not save private key. {}", err))?;
            }
        }

        Ok(CertificateProvider {
            account:account
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
            .merge(File::with_name("config/security.toml"))
            .map_err(|err| anyhow!("Could not get config from file {}", err))?;  
        
        let dns = config.get_table("dns")?;

        if let Some(token) = dns.get("api_token"){
            let val = token.clone();

            let provider = DnsProvider::new(&val.into_str()?)?;
            
            //provider.add_or_replace(&Zone::ByName("autobuild.cloud".into()), &ZoneRecord::TXT("test".into(), "This is a test".into())).await?;

            let p = CertificateProvider::new(provider, "discourse@gmail.com", "autobuild.cloud").await?;

            Ok(())
        } else { Err(anyhow!("Couldn't find api_key in config")) }

    }
}