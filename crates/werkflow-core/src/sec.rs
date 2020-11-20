use std::{sync::Arc, time::Duration};

use acme2_slim::{cert::SignedCertificate, Account};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cloudflare::framework::auth::Credentials;
use cloudflare::{
    endpoints::{
        dns::{self, DeleteDnsRecord, ListDnsRecordsParams},
        zone::{ListZones, ListZonesParams},
    },
    framework::{async_api::ApiClient, async_api::Client, Environment, HttpApiClientConfig},
};
use dns::ListDnsRecords;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum DnsProvider {
    Cloudflare,
}

#[async_trait]
pub trait DnsControllerClient {
    fn new_client(auth: Authentication) -> Self
    where
        Self: Sized;
    async fn get_id(&self, zone: &Zone) -> Result<String>;
    async fn get_record_id(&self, zone: &Zone, record: &ZoneRecord) -> Result<String>;
    async fn delete_record(&self, zone: &Zone, record: &ZoneRecord) -> Result<String>;
    async fn add_or_replace(&self, zone: &Zone, record: &ZoneRecord) -> Result<String>;
}
#[derive(Clone, Debug)]
pub enum Zone {
    ById(String),
    ByName(String),
}

#[derive(Clone, Debug)]
pub enum ZoneRecord {
    A(String, String),
    ProxiedA(String, String),
    TXT(String, String),
    // already existing records
    Id(String),
    Name(String),
}

#[derive(Clone)]
pub enum Authentication {
    ApiToken(String),
    UserPass(String, String),
    SslCertificate(String, String),
}

/// Implementation of a DNS provider using Cloudflare
#[async_trait]
impl DnsControllerClient for Client {
    //type ClientType = Client;

    async fn get_id(&self, zone: &Zone) -> Result<String> {
        match zone {
            Zone::ById(id) => Ok(id.clone()),
            Zone::ByName(name) => {
                let api_result = self
                    .request(&ListZones {
                        params: ListZonesParams {
                            name: Some(name.clone()),
                            ..Default::default()
                        },
                    })
                    .await?;

                match api_result.result.first() {
                    Some(zone) => Ok(zone.id.clone()),
                    None => Err(anyhow!("Error getting zone by name {}", name)),
                }
            }
        }
    }

    async fn get_record_id(&self, zone: &Zone, record: &ZoneRecord) -> Result<String> {
        let name = match record {
            ZoneRecord::Id(id) => return Ok(id.clone()),
            ZoneRecord::A(name, _) => Some(name.clone()),
            ZoneRecord::ProxiedA(name, _) => Some(name.clone()),
            ZoneRecord::TXT(name, _) => Some(name.clone()),
            ZoneRecord::Name(name) => Some(name.clone()),
        };

        let api_result = self
            .request(&ListDnsRecords {
                zone_identifier: &self.get_id(zone).await?,
                params: ListDnsRecordsParams {
                    name: name,
                    ..Default::default()
                },
            })
            .await?;

        match api_result.result.first() {
            Some(record) => Ok(record.id.clone()),
            None => Err(anyhow!("Error getting zone record.")),
        }
    }

    async fn delete_record(&self, zone: &Zone, record: &ZoneRecord) -> Result<String> {
        let id = self.get_record_id(zone, record).await?;

        let _api_result = self
            .request(&DeleteDnsRecord {
                zone_identifier: &self.get_id(zone).await?,
                identifier: &id,
            })
            .await?;

        Ok("Record deleted".into())
    }

    async fn add_or_replace(&self, zone: &Zone, record: &ZoneRecord) -> Result<String> {
        println!("Adding {:?} to {:?}", zone, record);
        let looked_up_zone = self.get_id(zone).await?;

        if let Ok(_record_id) = self.get_record_id(zone, record).await {
            self.delete_record(zone, record).await?;
        }

        let params = match record {
            ZoneRecord::A(host, ip) => dns::CreateDnsRecordParams {
                name: &host,
                content: dns::DnsContent::A {
                    content: ip.parse()?,
                },
                priority: None,
                proxied: None,
                ttl: None,
            },
            ZoneRecord::ProxiedA(host, ip) => dns::CreateDnsRecordParams {
                name: &host,
                content: dns::DnsContent::A {
                    content: ip.parse()?,
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
            _ => return Err(anyhow!("Invalid ZoneRecord type passed to add_or_replace.")),
        };

        let response = self
            .request(&dns::CreateDnsRecord {
                zone_identifier: &looked_up_zone,
                params: params,
            })
            .await
            .map_err(|err| anyhow!("Could not create record: {}", err))?;

        Ok(response.result.id)
    }

    fn new_client(auth: Authentication) -> Self
    where
        Self: Sized,
    {
        let creds = match auth {
            Authentication::ApiToken(token) => Credentials::UserAuthToken { token },
            Authentication::UserPass(email, key) => Credentials::UserAuthKey { email, key },
            Authentication::SslCertificate(_, _) => {
                panic!("SslCertificate not supported for Cloudflare Authentication")
            }
        };

        let api_client = Client::new(
            creds,
            HttpApiClientConfig::default(),
            Environment::Production,
        )
        .map_err(|err| anyhow!("Could not create Cloudflare API client: {}", err))
        .expect("to generate api client");

        api_client
    }
}

impl DnsProvider {
    pub fn new(&self, auth: Authentication) -> Arc<impl DnsControllerClient> {
        let client = match self {
            DnsProvider::Cloudflare => Arc::new(Client::new_client(auth)),
        };

        client
    }
}

pub struct CertificateProvider {
    account: Account,
    dns_provider: Option<DnsProvider>,
}

/// Certificate provider that uses ACME2 to create
/// certificates with a dns challenge.
/// TODO Add http challenge and integrate with the web feature.CertificateProvider

impl CertificateProvider {
    pub async fn order_with_dns(
        &mut self,
        provider: Arc<impl DnsControllerClient>,
        zone: &Zone,
        domains: Vec<String>,
    ) -> Result<Vec<SignedCertificate>> {
        let order = self
            .account
            .create_order(&domains)
            .await
            .map_err(|err| anyhow!("Error creating LetsEncrypt Order {}", err))?;
        let mut certificates: Vec<SignedCertificate> = Vec::new();
        for challenge in order.get_dns_challenges() {
            let signature = challenge
                .signature()
                .map_err(|err| anyhow!("Could not get signature. {}", err))?;

            let domain_name = &challenge.domain().expect("Domain name not in challenge");

            provider
                .add_or_replace(
                    zone,
                    &ZoneRecord::TXT(format!("_acme-challenge.{}", domain_name).into(), signature),
                )
                .await?;

            std::thread::sleep(Duration::from_secs(30));

            challenge
                .validate(&self.account, Duration::from_secs(10))
                .await
                .map_err(|err| anyhow!("Validation failed {}", err))?;

            let signer = self.account.certificate_signer();

            let cert = signer.sign_certificate(&order).await.unwrap();

            certificates.push(cert);
        }

        Ok(certificates)
    }
    pub async fn new(_email: &str) -> Result<CertificateProvider> {
        use acme2_slim::Directory;

        let directory = Directory::lets_encrypt()
            .await
            .map_err(|err| anyhow!("Error creating LetsEncrypt directory: {}", err))?;

        let account = directory
            .account_registration()
            //.email(email)
            .register()
            .await
            .map_err(|err| anyhow!("Error registering LetsEncrypt directory: {}", err))?;

        Ok(CertificateProvider {
            account: account,
            dns_provider: None,
        })
    }
}

#[cfg(test)]
mod test {
    use super::{CertificateProvider, DnsProvider, Zone};
    use crate::sec::Authentication;
    use anyhow::{anyhow, Result};
    use config::Config;
    use config::File;

    #[tokio::test(threaded_scheduler)]
    async fn test() -> Result<()> {
        let mut config = Config::new();

        config.merge(File::with_name("config/security.toml"))?;

        let dns = config.get_table("dns")?;

        if let Some(token) = dns.get("api_token") {
            let val = token.clone();

            let mut p = CertificateProvider::new("discourse@gmail.com").await?;

            let domains = vec!["test3.autobuild.cloud".into()];

            let certs = p
                .order_with_dns(
                    DnsProvider::Cloudflare
                        .new(Authentication::ApiToken(val.into_str()?.to_string()))
                        .clone(),
                    &Zone::ByName("autobuild.cloud".into()),
                    domains.clone(),
                )
                .await?;

            for (i, cert) in certs.iter().enumerate() {
                cert.save_signed_certificate(format!("config/{}.cer", &domains[i]))
                    .await
                    .map_err(|err| anyhow!("Could not save certificate: {}", err))?;
                cert.save_private_key(format!("config/{}.key", &domains[i]))
                    .await
                    .map_err(|err| anyhow!("Could not save private_key: {}", err))?;
            }

            Ok(())
        } else {
            Err(anyhow!("Couldn't find api_key in config"))
        }
    }
}
