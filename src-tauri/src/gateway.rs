//! Loopback session gateway. CURRENTLY UNUSED — kept for reference.
//!
//! This was written for the approach where the client is pointed at a local
//! stub through launch arguments. SUPER PEOPLE resolves real bravohotel.io
//! hostnames instead, so redirection happens in `hosts.rs` and the real
//! backend answers those names. Nothing calls this today; it is kept because
//! it works and the two approaches may both be wanted later.
//!
//! Binds 127.0.0.1 on an OS-assigned port and answers the client's
//! session-create request. Only loopback is bound, only the one path is
//! served, and everything else gets a 404.

#![allow(dead_code)]

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::error::Result;

const CREATE_SESSION_PATH: &str = "/rest/auth/session/create";

pub struct Gateway {
    pub create_session_url: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Gateway {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop();
    }
}

fn session_xml(ticket: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            "<createsessionresponse>",
            "<country>DE</country>",
            "<sessionid>{}</sessionid>",
            "<status>SUCCESS</status>",
            "</createsessionresponse>"
        ),
        ticket
    )
}

async fn handle(req: Request<hyper::body::Incoming>, ticket: Arc<String>)
    -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible>
{
    let path = req.uri().path().to_owned();

    let response = if path == CREATE_SESSION_PATH {
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/xml; charset=utf-8")
            .header("Cache-Control", "no-store")
            .header("X-Content-Type-Options", "nosniff")
            .body(Full::new(Bytes::from(session_xml(&ticket))))
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(Full::new(Bytes::from("not found")))
    };

    Ok(response.expect("static response builds"))
}

/// Starts the gateway on a free loopback port and returns its URL.
pub async fn start(ticket: String) -> Result<Gateway> {
    // Port 0 => the OS picks a free one, so two launchers never collide.
    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();

    let (tx, mut rx) = oneshot::channel::<()>();
    let ticket = Arc::new(ticket);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _peer)) = accepted else { continue };
                    let ticket = ticket.clone();
                    tokio::spawn(async move {
                        let io = TokioIo::new(stream);
                        let service = service_fn(move |req| handle(req, ticket.clone()));
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, service)
                            .await;
                    });
                }
            }
        }
    });

    Ok(Gateway {
        create_session_url: format!("http://127.0.0.1:{port}{CREATE_SESSION_PATH}"),
        shutdown: Some(tx),
    })
}
