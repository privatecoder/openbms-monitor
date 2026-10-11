//! Raw link used by both the real serial/TCP transport and the bootloader simulator.

use crate::Result;
use openbms_transport::Connection;
use std::time::Duration;

pub trait Link {
    fn clear_input(&mut self) -> Result<()>;
    fn write_raw(&mut self, bytes: &[u8]) -> Result<()>;
    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize>;
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

impl Link for Connection {
    fn clear_input(&mut self) -> Result<()> {
        Ok(Connection::clear_input(self)?)
    }
    fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        Ok(Connection::write_raw(self, bytes)?)
    }
    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize> {
        Ok(Connection::read_raw(self, out)?)
    }
}
