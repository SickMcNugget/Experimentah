// pub mod mongo {}
pub mod models;
pub mod schema;

pub mod prometheus {
    use std::{
        error::Error,
        str::FromStr,
        time::{SystemTime, UNIX_EPOCH},
    };

    use reqwest::{Client, Url};

    pub struct Prometheus {
        client: Client,
        url: Url,
    }

    pub struct PrometheusQuery {
        query_string: String,
        start_time: u64,
        end_time: u64,
        step: f64,
    }

    impl PrometheusQuery {
        pub fn build(
            query_string: String,
            start_time: SystemTime,
            end_time: SystemTime,
            step: f64,
        ) -> Result<Self, Box<dyn Error>> {
            let start_time = start_time.duration_since(UNIX_EPOCH)?.as_secs_f64().floor() as u64;
            let end_time = end_time.duration_since(UNIX_EPOCH)?.as_secs_f64().ceil() as u64;

            let prometheus_query = PrometheusQuery {
                query_string,
                start_time,
                end_time,
                step,
            };
            Ok(prometheus_query)
        }
    }

    impl Prometheus {
        pub fn build(url: String) -> Result<Self, Box<dyn Error>> {
            let parsed_url = Url::from_str(&url)?;
            let client = Client::new();

            let prometheus = Prometheus {
                client,
                url: parsed_url,
            };

            Ok(prometheus)
        }

        pub async fn health_check(&self) -> Result<(), Box<dyn Error>> {
            let healthy_endpoint = self.url.join("/-/healthy").unwrap();
            println!("Health endpoint: {}", healthy_endpoint.as_str());
            let response = self
                .client
                .get(self.url.join("/-/healthy").unwrap())
                .send()
                .await?;

            if let Err(e) = response.error_for_status() {
                return Err(e.into());
            }

            Ok(())
        }

        pub async fn query(&self, query: PrometheusQuery) -> Result<(), Box<dyn Error>> {
            Ok(())
        }
    }
}
