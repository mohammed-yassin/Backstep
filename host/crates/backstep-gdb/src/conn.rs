//! Connections to GDB: TCP and the standard streams.

use std::io;

use crate::Connection;

/// GDB over TCP (`target remote localhost:<port>`).
pub struct TcpConnection {
    stream: std::net::TcpStream,
    pending: std::collections::VecDeque<u8>,
}

impl TcpConnection {
    /// Wrap an accepted connection.
    #[must_use]
    pub fn new(stream: std::net::TcpStream) -> Self {
        let _ = stream.set_nodelay(true);
        TcpConnection {
            stream,
            pending: std::collections::VecDeque::new(),
        }
    }

    fn fill(&mut self) -> io::Result<usize> {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        let n = self.stream.read(&mut buf)?;
        self.pending.extend(&buf[..n]);
        Ok(n)
    }
}

impl Connection for TcpConnection {
    fn read_byte(&mut self) -> io::Result<Option<u8>> {
        if self.pending.is_empty() && self.fill()? == 0 {
            return Ok(None);
        }
        Ok(self.pending.pop_front())
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.stream.write_all(data)?;
        self.stream.flush()
    }

    fn interrupt_requested(&mut self) -> bool {
        if self.stream.set_nonblocking(true).is_ok() {
            let _ = self.fill();
            let _ = self.stream.set_nonblocking(false);
        }
        if let Some(i) = self.pending.iter().position(|&b| b == 0x03) {
            self.pending.remove(i);
            return true;
        }
        false
    }
}

/// GDB over the standard streams (`target remote | backstep gdb --stdio
/// ...`). A run cannot be interrupted this way.
pub struct StdioConnection {
    input: io::StdinLock<'static>,
    output: io::StdoutLock<'static>,
}

impl StdioConnection {
    /// Use this process's stdin and stdout.
    #[must_use]
    pub fn new() -> Self {
        StdioConnection {
            input: io::stdin().lock(),
            output: io::stdout().lock(),
        }
    }
}

impl Default for StdioConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl Connection for StdioConnection {
    fn read_byte(&mut self) -> io::Result<Option<u8>> {
        use std::io::Read;
        let mut b = [0u8; 1];
        match self.input.read(&mut b)? {
            0 => Ok(None),
            _ => Ok(Some(b[0])),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.output.write_all(data)?;
        self.output.flush()
    }

    fn interrupt_requested(&mut self) -> bool {
        false
    }
}
