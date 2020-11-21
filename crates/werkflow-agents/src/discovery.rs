use trust_dns_resolver::AsyncResolver;

use std::{time::Duration, net::IpAddr};
use std::str::FromStr;
//_service._proto.name. TTL IN SRV priority weight port target.

#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String
}

impl Service {
    pub async fn lookup(service: &str) -> Vec<Service> {
        let mut rvec = Vec::default();

        let resolver = AsyncResolver::tokio_from_system_conf().await.unwrap();

        let resolv = resolver.srv_lookup(service).await.unwrap();

        for n in resolv.iter() {
            rvec.push(Service {
                name: service.to_string(),
                port: n.port(),
                priority: n.priority(),
                target: n.target().to_string(),
                weight: n.weight(),
            });
        }
        rvec
    }
}

#[tokio::test]
async fn test_service (){
    let services = Service::lookup("_srv._tcp.autobuild.cloud.").await;
    assert_eq!(services.len(), 1);
}