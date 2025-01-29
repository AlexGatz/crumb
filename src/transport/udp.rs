use crate::util::config::Config;
use std::io::{self, Error};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

pub struct Client {
    socket: UdpSocket,
}

impl Client {
    pub fn init(conf: &Config) -> io::Result<Client> {
        let addr = format!("{}:{}", conf.host, conf.port);
        // Apparenting this is correct?!
        let socket = UdpSocket::bind("[::]:0")?;
        socket.connect(&addr)?;

        Ok(Client { socket })
    }

    pub fn send(&self, data: &[u8]) -> io::Result<usize> {
        self.socket.send(data)
    }

    pub fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.recv(buffer)
    }

    pub fn close(self) {
        drop(self.socket);
    }
}

pub struct Server {
    socket: UdpSocket,
}

impl Server {
    pub fn init(conf: &Config) -> io::Result<Server> {
        // Support IPv4 and IPv6 on Linux
        let addr = format!("[::]:{}", conf.port);
        let socket = UdpSocket::bind(&addr)?;

        Ok(Server { socket })
    }

    pub fn send_to<A: ToSocketAddrs>(&self, data: &[u8], dest: A) -> io::Result<usize> {
        self.socket.send_to(data, dest)
    }

    pub fn receive_from(&self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buffer)
    }

    pub fn close(self) {
        drop(self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*; // Import the Client, Server, and Config structs
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_client_server_interaction() -> io::Result<()> {
        let server_conf = Config {
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };

        // Spawn the server in a separate thread
        let server_handle = thread::spawn(move || {
            let server = Server::init(&server_conf).expect("Failed to initialize server");

            let mut buffer = [0u8; 1024];
            let (bytes_received, client_addr) = server
                .receive_from(&mut buffer)
                .expect("Failed to receive data");

            let message = String::from_utf8_lossy(&buffer[..bytes_received]);
            assert_eq!(message, "Hello, Server!");

            // Send response back to the client
            server
                .send_to(b"Hello, Client!", client_addr)
                .expect("Failed to send response");

            server.close();
        });

        // Allow the server some time to start
        thread::sleep(Duration::from_millis(100));

        // Configure and start the client
        let client_conf = Config {
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };

        let client = Client::init(&client_conf).expect("Failed to initialize client");

        // Send a message to the server
        client.send(b"Hello, Server!").expect("Failed to send data");

        // Prepare a buffer for the server's response
        let mut buffer = [0u8; 1024];
        let bytes_received = client
            .receive(&mut buffer)
            .expect("Failed to receive response");

        let response = String::from_utf8_lossy(&buffer[..bytes_received]);
        assert_eq!(response, "Hello, Client!");

        client.close();

        // Wait for the server thread to complete
        server_handle.join().expect("Server thread panicked");

        Ok(())
    }
}
