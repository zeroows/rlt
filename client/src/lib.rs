use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

pub const AD4M_PROXY_SERVER: &str = "http://proxy.ad4m.dev";
pub const LOCAL_HOST: &str = "127.0.0.1";

#[derive(Debug, Serialize, Deserialize)]
struct ProxyResponse {
    id: String,
    port: u16,
    max_conn_count: u8,
    url: String,
}

#[derive(Clone)]
pub struct TunnelServerInfo {
    pub host: String,
    pub port: u16,
    pub max_conn_count: u8,
    pub url: String,
}

/// Open tunnels directly between server and localhost
pub async fn open_tunnel(
    server: Option<&str>,
    subdomain: Option<&str>,
    local_host: Option<&str>,
    local_port: u16,
) -> Result<(String, JoinHandle<()>), Box<dyn std::error::Error>> {
    let tunnel_info = get_tunnel_endpoint(server, subdomain).await?;

    // TODO check the connect is failed and restart the proxy.
    let handle = tunnel_to_endpoint(tunnel_info.clone(), local_host, local_port).await;

    Ok((tunnel_info.url, handle))
}

pub async fn get_tunnel_endpoint(
    server: Option<&str>,
    subdomain: Option<&str>,
) -> Result<TunnelServerInfo, Box<dyn std::error::Error>> {
    let server = server.unwrap_or(AD4M_PROXY_SERVER);
    let assigned_domain = subdomain.unwrap_or("?new");
    let uri = format!("{}/{}", server, assigned_domain);
    println!("Request for assign domain: {}", uri);

    let resp = reqwest::get(uri).await?.json::<ProxyResponse>().await?;
    println!("{:#?}", resp);

    let parts = server.split("//").collect::<Vec<&str>>();
    let host = parts[1];

    let tunnel_info = TunnelServerInfo {
        host: host.to_string(),
        port: resp.port,
        max_conn_count: resp.max_conn_count,
        url: resp.url,
    };

    Ok(tunnel_info)
}

pub async fn tunnel_to_endpoint(
    server: TunnelServerInfo,
    local_host: Option<&str>,
    local_port: u16,
) -> JoinHandle<()> {
    let server_host = server.host;
    let server_port = server.port;
    let local_host = local_host.unwrap_or(LOCAL_HOST).to_string();

    let handle = tokio::spawn(async move {
        let counter = Arc::new(Mutex::new(0));

        loop {
            sleep(Duration::from_millis(600)).await;

            let mut locked_counter = counter.lock().await;
            if *locked_counter < server.max_conn_count {
                println!("Create a new proxy connection.");
                *locked_counter += 1;

                let server_host = server_host.clone();
                let local_host = local_host.clone();
                let counter2 = Arc::clone(&counter);

                tokio::spawn(async move {
                    let result = handle_connection(
                        server_host,
                        server_port,
                        local_host,
                        local_port,
                        counter2,
                    )
                    .await;
                    println!("Connection result: {:?}", result);
                });
            }
        }
    });

    handle
}

async fn handle_connection(
    remote_host: String,
    remote_port: u16,
    local_host: String,
    local_port: u16,
    counter: Arc<Mutex<u8>>,
) -> Result<()> {
    let remote_stream = TcpStream::connect(format!("{}:{}", remote_host, remote_port)).await?;
    let local_stream = TcpStream::connect(format!("{}:{}", local_host, local_port)).await?;

    proxy(remote_stream, local_stream, counter).await?;
    Ok(())
}

/// Copy data mutually between two read/write streams.
async fn proxy<S1, S2>(stream1: S1, stream2: S2, counter: Arc<Mutex<u8>>) -> io::Result<()>
where
    S1: AsyncRead + AsyncWrite + Unpin,
    S2: AsyncRead + AsyncWrite + Unpin,
{
    let (mut s1_read, mut s1_write) = io::split(stream1);
    let (mut s2_read, mut s2_write) = io::split(stream2);
    tokio::select! {
        res = io::copy(&mut s1_read, &mut s2_write) => res,
        res = io::copy(&mut s2_read, &mut s1_write) => res,
    }?;
    let mut locked_counter = counter.lock().await;

    println!("counter: {}", *locked_counter);
    *locked_counter -= 1;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
