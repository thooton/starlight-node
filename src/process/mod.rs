mod message;
mod handle;
mod mailbox;
pub use message::Message;
pub use handle::Handle;
pub use mailbox::Mailbox;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use crate::network::Endpoint;
use crate::util::{self, Error, UninitVec};
use crate::{log_error, log_warn};

const SLEEP_MS_BEFORE_RETRY: u64 = 20;

pub fn sleep(dur: Duration) {
    std::thread::sleep(dur);
}

pub trait Process {
    const NAME: &'static str;
    const RESTART_ON_CRASH: bool;
    fn run(&mut self, mailbox: Mailbox, handle: Handle) -> Result<(), Error>;
}

pub fn spawn<P: Process + Send + 'static>(mut process: P) -> Handle {
    let (tx, rx) = kanal::unbounded();
    let handle = Handle(tx.clone());
    thread::spawn(move || {
        let handle = Handle(tx);
        let mailbox = Mailbox(rx);
        loop {
            match process.run(mailbox.clone(), handle.clone()) {
                Ok(_) => break,
                Err(e) => {
                    if !P::RESTART_ON_CRASH {
                        break;
                    }
                    log_error!("process {} failed: {}", P::NAME, e);
                    thread::sleep(Duration::from_millis(SLEEP_MS_BEFORE_RETRY));
                    log_warn!("restarting process {}", P::NAME);
                }
            }
        }
        handle.deactivate();
    });
    handle
}

pub trait ProcessEndless {
    fn run(&mut self, mailbox: Mailbox, handle: Handle) -> !;
}

pub fn spawn_endless<P: ProcessEndless + Send + 'static>(mut process: P) -> Handle {
    let (tx, rx) = kanal::unbounded();
    let handle = Handle(tx.clone());
    thread::spawn(move || {
        let handle = Handle(tx);
        let mailbox = Mailbox(rx);
        process.run(mailbox, handle);
    });
    handle
}

fn recv_message(socket: &mut TcpStream) -> Result<(Handle, Message), Error> {
    let mut len = [0u8; 4];
    socket.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    // safety: no uninitialized bytes are read
    let mut buf = unsafe { Vec::uninit(len) };
    socket.read_exact(&mut buf)?;
    Ok(util::decode_from_slice(&buf)?)
}

fn send_message(socket: &mut TcpStream, msg: Message) -> Result<(), Error> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0u32.to_le_bytes());
    if util::encode_into_writer(&mut buf, &msg).is_err() {
        return Ok(())
    }
    let len = buf.len() as u32;
    buf[0..4].copy_from_slice(&len.to_le_bytes());
    socket.write_all(&mut buf)?;
    Ok(())
}

pub fn connect_remote(ep: Endpoint) -> Handle {
    let (tx, rx) = kanal::unbounded();
    let handle = Handle(tx.clone());
    thread::spawn(move || {
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                thread::sleep(Duration::from_millis(100));
            }
            let mut socket1 = match TcpStream::connect(ep.to_socket_addr()) {
                Ok(v) => v,
                Err(_) => continue
            };
            let mut socket2 = match socket1.try_clone() {
                Ok(v) => v,
                Err(_) => continue
            };
            let rx = rx.clone();
            thread::spawn(move || {
                loop {
                    let msg = match rx.recv() {
                        Ok(v) => v,
                        Err(_) => break
                    };
                    if send_message(&mut socket1, msg).is_err() {
                        break;
                    }
                }
            });
            loop {
                let (handle, msg) = match recv_message(&mut socket2) {
                    Ok(v) => v,
                    Err(_) => break
                };
                handle.send(msg);
            }
            tx.close();
        }
    });
    handle
}

