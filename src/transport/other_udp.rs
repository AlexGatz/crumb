use openssl::ssl::{SslAcceptor, SslConnector, SslFiletype, SslMethod, SslStream};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub use_dtls: bool,
}

#[derive(Debug)]
pub struct UdpStream {
    socket: UdpSocket,
}

impl UdpStream {
    pub fn new(socket: UdpSocket) -> Self {
        Self { socket }
    }
}

impl Read for UdpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.recv(buf)
    }
}

impl Write for UdpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.socket.send(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub trait DatagramStream: Read + Write {
    fn set_timeout(&self, duration: Option<Duration>) -> io::Result<()>;
    fn peer_addr(&self) -> io::Result<SocketAddr>;
}

impl DatagramStream for UdpStream {
    fn set_timeout(&self, duration: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(duration)?;
        self.socket.set_write_timeout(duration)
    }
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.socket.peer_addr()
    }
}

impl DatagramStream for SslStream<UdpStream> {
    fn set_timeout(&self, duration: Option<Duration>) -> io::Result<()> {
        self.get_ref().set_timeout(duration)
    }
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.get_ref().peer_addr()
    }
}

pub struct Client {
    transport: Box<dyn DatagramStream + Send>,
}

impl Client {
    pub fn init(conf: &Config) -> io::Result<Self> {
        let addr = format!("{}:{}", conf.host, conf.port);
        let socket = UdpSocket::bind("[::]:0")?;
        socket.connect(&addr)?;

        let transport: Box<dyn DatagramStream + Send> = if conf.use_dtls {
            let connector = SslConnector::builder(SslMethod::dtls()).unwrap().build();
            let ssl = connector
                .configure()
                .unwrap()
                .into_ssl("localhost")
                .unwrap();
            let mut ssl_stream = SslStream::new(ssl, UdpStream::new(socket.try_clone()?)).unwrap();

            // Perform handshake
            if let Err(e) = ssl_stream.connect() {
                eprintln!("DTLS handshake failed: {}", e);
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "DTLS handshake failed",
                ));
            }

            println!("DTLS handshake successful with server");
            Box::new(ssl_stream)
        } else {
            Box::new(UdpStream::new(socket))
        };

        Ok(Self { transport })
    }

    pub fn send(&mut self, data: &[u8]) -> io::Result<usize> {
        self.transport.write(data)
    }

    pub fn receive(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.transport.read(buffer)
    }

    pub fn close(self) {
        drop(self.transport);
    }
}

pub struct Server {
    socket: UdpSocket,
    dtls_acceptor: Option<SslAcceptor>,
}

impl Server {
    pub fn init(conf: &Config) -> io::Result<Self> {
        let addr = format!("[::]:{}", conf.port);
        let socket = UdpSocket::bind(&addr)?;

        let dtls_acceptor = if conf.use_dtls {
            let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::dtls()).unwrap();
            acceptor
                .set_private_key_file("key.pem", SslFiletype::PEM)
                .unwrap();
            acceptor.set_certificate_chain_file("cert.pem").unwrap();
            Some(acceptor.build())
        } else {
            None
        };

        Ok(Self {
            socket,
            dtls_acceptor,
        })
    }

    pub fn handle_client(&self) -> io::Result<()> {
        let mut buffer = [0u8; 4096];
        let (size, client_addr) = self.socket.recv_from(&mut buffer)?;
        println!("Received {} bytes from {}", size, client_addr);

        let mut transport: Box<dyn DatagramStream + Send> =
            if let Some(acceptor) = &self.dtls_acceptor {
                let udp_stream = UdpStream::new(self.socket.try_clone()?); // Wrap UdpSocket
                match acceptor.accept(udp_stream) {
                    Ok(ssl_stream) => {
                        println!("DTLS handshake successful with {}", client_addr);
                        Box::new(ssl_stream)
                    }
                    Err(e) => {
                        eprintln!("DTLS handshake failed: {}", e);
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "DTLS handshake failed",
                        ));
                    }
                }
            } else {
                Box::new(UdpStream::new(self.socket.try_clone()?))
            };

        transport.write_all(&buffer[..size])?;
        println!("Response sent to {}", client_addr);
        Ok(())
    }

    pub fn close(self) {
        drop(self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn create_test_config(use_dtls: bool) -> Config {
        Config {
            host: "127.0.0.1".to_string(),
            port: if use_dtls { 8081 } else { 8080 },
            use_dtls,
        }
    }

    #[test]
    fn test_udp_client_server() -> io::Result<()> {
        let config = create_test_config(false); // Cleartext UDP
        let server_config = config.clone(); // Clone for the server

        let server_handle = thread::spawn(move || {
            let server = Server::init(&server_config).expect("Failed to start server");
            let mut buffer = [0u8; 1024];

            let (size, client_addr) = server
                .socket
                .recv_from(&mut buffer)
                .expect("Server failed to receive");
            let received_msg = String::from_utf8_lossy(&buffer[..size]);
            assert_eq!(received_msg, "Hello, Server!");

            server
                .socket
                .send_to(b"Hello, Client!", client_addr)
                .expect("Server failed to send");
        });

        thread::sleep(Duration::from_millis(100)); // Allow server to start

        let mut client = Client::init(&config).expect("Failed to start client");
        client
            .send(b"Hello, Server!")
            .expect("Client failed to send");

        let mut buffer = [0u8; 1024];
        let size = client
            .receive(&mut buffer)
            .expect("Client failed to receive");
        let received_msg = String::from_utf8_lossy(&buffer[..size]);
        assert_eq!(received_msg, "Hello, Client!");

        client.close();
        server_handle.join().expect("Server thread panicked");

        Ok(())
    }

    #[test]
    fn test_dtls_client_server() -> io::Result<()> {
        let config = create_test_config(true); // DTLS
        let server_config = config.clone(); // Clone for the server

        let server_handle = thread::spawn(move || {
            let server = Server::init(&server_config).expect("Failed to start server");
            server
                .handle_client()
                .expect("Server failed to handle DTLS client");
        });

        thread::sleep(Duration::from_millis(100)); // Allow server to start

        let mut client = Client::init(&config).expect("Failed to start DTLS client");
        client
            .send(b"Hello, Server!")
            .expect("Client failed to send");

        let mut buffer = [0u8; 1024];
        let size = client
            .receive(&mut buffer)
            .expect("Client failed to receive");
        let received_msg = String::from_utf8_lossy(&buffer[..size]);
        assert_eq!(received_msg, "Hello, Client!");

        client.close();
        server_handle.join().expect("Server thread panicked");

        Ok(())
    }
}
