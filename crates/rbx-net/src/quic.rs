use crate::transport::Connection;
use crate::wire::Channel;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc as tmpsc;

pub const ALPN: &[u8] = b"rbx/1";
const MAX_FRAME: usize = 16 << 20;

#[derive(Debug, thiserror::Error)]
pub enum QuicError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    #[error("certificate: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("connect: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("connection: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("{0}")]
    Other(String),
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rbx-net")
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("cannot start the network runtime: {e}"),
        }
    })
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn transport() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(5)));
    if let Ok(idle) = quinn::IdleTimeout::try_from(Duration::from_secs(30)) {
        t.max_idle_timeout(Some(idle));
    }
    t.datagram_receive_buffer_size(Some(4 << 20));
    Arc::new(t)
}

pub struct Certificate {
    pub der: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

impl Certificate {
    pub fn self_signed(names: &[&str]) -> Result<Certificate, QuicError> {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let cert = rcgen::generate_simple_self_signed(names)?;
        Ok(Certificate {
            der: cert.cert.der().clone(),
            key: PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
        })
    }

    pub fn hash(&self) -> [u8; 32] {
        let digest = Sha256::digest(self.der.as_ref());
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }
}

pub struct QuicServer {
    endpoint: quinn::Endpoint,
    incoming: mpsc::Receiver<QuicConnection>,
    cert_hash: [u8; 32],
}

impl QuicServer {
    pub fn bind(addr: SocketAddr, cert: Certificate) -> Result<QuicServer, QuicError> {
        let cert_hash = cert.hash();
        let mut tls = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(vec![cert.der], PrivateKeyDer::Pkcs8(cert.key))?;
        tls.alpn_protocols = vec![ALPN.to_vec()];
        tls.max_early_data_size = 0;
        let crypto =
            QuicServerConfig::try_from(tls).map_err(|e| QuicError::Other(e.to_string()))?;
        let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        config.transport_config(transport());
        let rt = runtime();
        let endpoint = {
            let _guard = rt.enter();
            quinn::Endpoint::server(config, addr)?
        };
        let (tx, incoming) = mpsc::channel();
        let ep = endpoint.clone();
        rt.spawn(async move {
            while let Some(incoming) = ep.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else {
                        return;
                    };
                    let Ok((send, recv)) = conn.accept_bi().await else {
                        return;
                    };
                    let _ = tx.send(QuicConnection::spawn(conn, send, recv));
                });
            }
        });
        Ok(QuicServer {
            endpoint,
            incoming,
            cert_hash,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, QuicError> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn cert_hash(&self) -> [u8; 32] {
        self.cert_hash
    }

    pub fn accept(&self) -> Option<QuicConnection> {
        self.incoming.try_recv().ok()
    }

    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"server closed");
    }
}

#[derive(Debug)]
struct PinnedHash {
    hash: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedHash {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let digest = Sha256::digest(end_entity.as_ref());
        if digest.as_slice() == self.hash {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "server certificate does not match the pinned hash".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

pub fn connect(addr: SocketAddr, cert_hash: [u8; 32]) -> Result<QuicConnection, QuicError> {
    let provider = provider();
    let verifier = Arc::new(PinnedHash {
        hash: cert_hash,
        algorithms: provider.signature_verification_algorithms,
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(|e| QuicError::Other(e.to_string()))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport());
    let bind: SocketAddr = if addr.is_ipv6() {
        "[::]:0"
            .parse()
            .map_err(|_| QuicError::Other("bad bind address".into()))?
    } else {
        "0.0.0.0:0"
            .parse()
            .map_err(|_| QuicError::Other("bad bind address".into()))?
    };
    let rt = runtime();
    rt.block_on(async move {
        let mut endpoint = quinn::Endpoint::client(bind)?;
        endpoint.set_default_client_config(config);
        let conn = endpoint.connect(addr, "localhost")?.await?;
        let (mut send, recv) = conn.open_bi().await.map_err(QuicError::Connection)?;
        send.write_all(&0u32.to_le_bytes())
            .await
            .map_err(|e| QuicError::Other(e.to_string()))?;
        Ok(QuicConnection::spawn_with_endpoint(
            conn,
            send,
            recv,
            Some(endpoint),
        ))
    })
}

pub struct QuicConnection {
    inbox: mpsc::Receiver<Vec<u8>>,
    out: tmpsc::UnboundedSender<(Channel, Vec<u8>)>,
    closed: Arc<AtomicBool>,
    conn: quinn::Connection,
    _endpoint: Option<quinn::Endpoint>,
    pub sent_bytes: u64,
}

impl QuicConnection {
    fn spawn(conn: quinn::Connection, send: quinn::SendStream, recv: quinn::RecvStream) -> Self {
        Self::spawn_with_endpoint(conn, send, recv, None)
    }

    fn spawn_with_endpoint(
        conn: quinn::Connection,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
        endpoint: Option<quinn::Endpoint>,
    ) -> Self {
        let rt = runtime();
        let (in_tx, inbox) = mpsc::channel::<Vec<u8>>();
        let (out, mut out_rx) = tmpsc::unbounded_channel::<(Channel, Vec<u8>)>();
        let closed = Arc::new(AtomicBool::new(false));

        let tx = in_tx.clone();
        let flag = closed.clone();
        rt.spawn(async move {
            let mut len = [0u8; 4];
            loop {
                if recv.read_exact(&mut len).await.is_err() {
                    break;
                }
                let n = u32::from_le_bytes(len) as usize;
                if n > MAX_FRAME {
                    break;
                }
                if n == 0 {
                    continue;
                }
                let mut buf = vec![0u8; n];
                if recv.read_exact(&mut buf).await.is_err() || tx.send(buf).is_err() {
                    break;
                }
            }
            flag.store(true, Ordering::Release);
        });

        let dconn = conn.clone();
        let flag = closed.clone();
        rt.spawn(async move {
            while let Ok(d) = dconn.read_datagram().await {
                if in_tx.send(d.to_vec()).is_err() {
                    break;
                }
            }
            flag.store(true, Ordering::Release);
        });

        let wconn = conn.clone();
        let flag = closed.clone();
        rt.spawn(async move {
            while let Some((channel, bytes)) = out_rx.recv().await {
                if channel == Channel::Unreliable
                    && wconn
                        .max_datagram_size()
                        .is_some_and(|max| bytes.len() <= max)
                    && wconn.send_datagram(bytes.clone().into()).is_ok()
                {
                    continue;
                }
                let len = (bytes.len() as u32).to_le_bytes();
                if send.write_all(&len).await.is_err() || send.write_all(&bytes).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
            flag.store(true, Ordering::Release);
        });

        QuicConnection {
            inbox,
            out,
            closed,
            conn,
            _endpoint: endpoint,
            sent_bytes: 0,
        }
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    pub fn rtt(&self) -> Duration {
        self.conn.rtt()
    }
}

impl Connection for QuicConnection {
    fn describe(&self) -> String {
        self.conn.remote_address().to_string()
    }

    fn send_bytes(&mut self, channel: Channel, bytes: Vec<u8>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.sent_bytes += bytes.len() as u64;
        if self.out.send((channel, bytes)).is_err() {
            self.closed.store(true, Ordering::Release);
        }
    }

    fn recv_bytes(&mut self) -> Option<Vec<u8>> {
        self.inbox.try_recv().ok()
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire) || self.conn.close_reason().is_some()
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.conn.close(0u32.into(), b"closed");
    }
}
