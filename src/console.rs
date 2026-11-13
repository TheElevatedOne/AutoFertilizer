//! The original plugin's remote console: a TCP client on 127.0.0.1:50382 that takes
//! `autofarm harvest|fert|remain|reload` and answers with `console <text>`. Only used when something
//! listens on that port at startup.

use std::{
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use crate::game::Command;

const ADDRESS: ([u8; 4], u16) = ([127, 0, 0, 1], 50382);

pub enum Request {
    Game(Command),
    Reload,
}

pub struct Console(Option<TcpStream>);

impl Console {
    pub fn connect() -> Self {
        let address = SocketAddr::from(ADDRESS);
        let stream = TcpStream::connect_timeout(&address, Duration::from_millis(300))
            .ok()
            .filter(|s| s.set_nonblocking(true).is_ok());
        Self(stream)
    }

    pub fn connected(&self) -> bool {
        self.0.is_some()
    }

    /// The next request, if one came in since the last call.
    pub fn poll(&mut self) -> Option<Result<Request, String>> {
        let stream = self.0.as_mut()?;
        let mut buf = [0u8; 1024];
        let n = match stream.read(&mut buf) {
            Ok(0) => {
                self.0 = None;
                return None;
            }
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => return None,
            Err(_) => {
                self.0 = None;
                return None;
            }
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        let mut words = text.split_whitespace();
        if words.next() != Some("autofarm") {
            return None;
        }
        Some(match words.next() {
            Some("harvest") => Ok(Request::Game(Command::Harvest)),
            Some("fert") => Ok(Request::Game(Command::Fertilize)),
            Some("remain") => Ok(Request::Game(Command::Remain)),
            Some("reload") => Ok(Request::Reload),
            other => Err(format!("invalid command: {}", other.unwrap_or(""))),
        })
    }

    pub fn send(&mut self, text: &str) {
        if let Some(stream) = self.0.as_mut()
            && stream.write_all(format!("console {text}").as_bytes()).is_err()
        {
            self.0 = None;
        }
    }
}
