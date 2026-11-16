//! Mach messages of bytes and ports: what a message of the protocol is on the wire, sent without
//! waiting and received with its sender's audit token.
//!
//! A message is a header whose id is the protocol's number, the ports it carries as port
//! descriptors, each a send right moved to the receiver, and its bytes: inline when they are few,
//! and above [`INLINE_LIMIT`] as out-of-line memory, which the kernel maps into the receiver
//! rather than copying through the message (a text a program edits may be megabytes). Every
//! message received carries the audit token of the process that sent it, which the kernel writes
//! and no sender can forge: how WaOS knows which process said hello.
//!
//! The invariant a later editor will break: every right and every page a message carries is
//! owned by exactly one side. A right moved in a message that was sent is the receiver's; a
//! message that could not be sent gives back what it held; a message received hands each right
//! to a [`SendRight`] and deallocates its out-of-line memory once it is copied; and a message of
//! another protocol is destroyed whole.

use std::io;

use mach2::message::{
    MACH_MSG_OOL_DESCRIPTOR, MACH_MSG_PORT_DESCRIPTOR, MACH_MSG_SUCCESS, MACH_MSG_TIMEOUT_NONE,
    MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MOVE_SEND, MACH_MSG_VIRTUAL_COPY, MACH_MSGH_BITS,
    MACH_MSGH_BITS_COMPLEX, MACH_RCV_LARGE, MACH_RCV_MSG, MACH_RCV_TOO_LARGE,
    MACH_RCV_TRAILER_AUDIT, MACH_SEND_MSG, MACH_SEND_TIMED_OUT, MACH_SEND_TIMEOUT, mach_msg,
    mach_msg_destroy, mach_msg_header_t,
};
use mach2::notify::MACH_NOTIFY_NO_SENDERS;
use mach2::port::MACH_PORT_NULL;
use mach2::traps::mach_task_self;
use mach2::vm::mach_vm_deallocate;

use super::ports::{ReceiveRight, SendRight, checked, deallocate, failure};

/// The most bytes a message carries inline; more go out of line.
pub(crate) const INLINE_LIMIT: usize = 64 * 1024;

/// The header, then the count of descriptors when there are any.
const HEADER_SIZE: usize = size_of::<mach_msg_header_t>();
const BODY_SIZE: usize = 4;
/// A port descriptor and an out-of-line one, as a 64-bit process writes and reads them.
const PORT_DESCRIPTOR_SIZE: usize = 12;
const OOL_DESCRIPTOR_SIZE: usize = 16;
/// The audit trailer the kernel appends: type, size, sequence number, the sender's security
/// token, then its audit token.
const AUDIT_TRAILER_SIZE: usize = 52;
const AUDIT_TOKEN_OFFSET: usize = 20;

/// Asks for the sender's audit token in every message received: `MACH_RCV_TRAILER_TYPE(0) |
/// MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT)`.
const AUDIT_TRAILER: i32 = (MACH_RCV_TRAILER_AUDIT as i32) << 24;

/// What arrived on a port.
#[derive(Debug)]
pub enum Arrival {
    Message(Received),
    /// A message of another protocol than the one asked for, from a WaOS or a window process of
    /// another build: its protocol, from its id, which is read before anything of it, and what it
    /// brought, its body unread, so the receiver can say which and answer it.
    OtherProtocol {
        protocol: u32,
        received: Received,
    },
    /// No send right to the port is left: the process that held the last one let it go, or
    /// ended (`ReceiveRight::notify_when_no_senders`).
    NoSenders,
}

/// A message received: its bytes, the ports it carried, now this process's, and who sent it.
#[derive(Debug)]
pub struct Received {
    pub bytes: Vec<u8>,
    pub ports: Vec<SendRight>,
    pub sender: AuditToken,
}

/// The audit token of the process that sent a message, as the kernel wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditToken(pub [u32; 8]);

impl AuditToken {
    /// The process's id (`audit_token_to_pid`).
    pub fn pid(&self) -> i32 {
        self.0[5] as i32
    }

    /// The token's bytes, as Security's `kSecGuestAttributeAudit` takes them.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().flat_map(|word| word.to_ne_bytes()).collect()
    }
}

impl SendRight {
    /// Sends `bytes` and `ports` as one message of the protocol `protocol`, without waiting for
    /// room in the port's queue: a queue that is full, or a port that is gone, fails at once.
    /// The ports are the receiver's once it is sent, and let go of when it is not.
    pub fn send(&self, protocol: u32, bytes: &[u8], ports: Vec<SendRight>) -> io::Result<()> {
        let inline = bytes.len() <= INLINE_LIMIT;
        let names: Vec<u32> = ports.into_iter().map(SendRight::into_name).collect();
        let mut message = Outgoing::new(self.name(), protocol, &names, bytes, inline);
        // SAFETY: the buffer holds a message of the size its header gives, as written above; an
        // out-of-line descriptor points into `bytes`, which outlives the call.
        let sent = unsafe {
            mach_msg(
                message.header(),
                MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                message.size(),
                0,
                MACH_PORT_NULL,
                0,
                MACH_PORT_NULL,
            )
        };
        if sent == MACH_MSG_SUCCESS {
            return Ok(());
        }
        if sent == MACH_SEND_TIMED_OUT {
            // SAFETY: a message that timed out comes back as if received, holding its rights.
            unsafe { mach_msg_destroy(message.header()) };
        } else {
            names.into_iter().for_each(deallocate);
        }
        checked(sent, "mach_msg send")
    }
}

impl ReceiveRight {
    /// Waits for what arrives next on the port. A message of another protocol than `protocol`
    /// arrives as [`Arrival::OtherProtocol`], its body for nobody to read.
    pub fn receive(&self, protocol: u32) -> io::Result<Arrival> {
        let mut buffer = ReceiveBuffer::sized(INLINE_LIMIT + 1024);
        loop {
            // SAFETY: the buffer is aligned for a header and as large as its size says; with
            // `MACH_RCV_LARGE` a larger message stays queued and says how large it is.
            let received = unsafe {
                mach_msg(
                    buffer.header(),
                    MACH_RCV_MSG | MACH_RCV_LARGE | AUDIT_TRAILER,
                    0,
                    buffer.capacity(),
                    self.name(),
                    MACH_MSG_TIMEOUT_NONE,
                    MACH_PORT_NULL,
                )
            };
            match received {
                MACH_MSG_SUCCESS => return buffer.read(protocol),
                MACH_RCV_TOO_LARGE => {
                    let size = buffer.header_read().msgh_size as usize;
                    buffer = ReceiveBuffer::sized(size + AUDIT_TRAILER_SIZE);
                }
                failed => return Err(failure(failed, "mach_msg receive")),
            }
        }
    }
}

/// A message being written: header, descriptors, then the inline bytes' count and the bytes.
struct Outgoing(Vec<u32>);

impl Outgoing {
    fn new(port: u32, protocol: u32, ports: &[u32], bytes: &[u8], inline: bool) -> Outgoing {
        let descriptors = ports.len() + usize::from(!inline);
        let complex = descriptors > 0;
        let inline_bytes = if inline { bytes } else { &[] };
        let mut words = Words::default();
        words.header(port, protocol, complex);
        if complex {
            words.push(descriptors as u32);
        }
        for name in ports {
            words.port_descriptor(*name);
        }
        if !inline {
            words.ool_descriptor(bytes);
        }
        words.push(inline_bytes.len() as u32);
        words.bytes(inline_bytes);
        let mut message = Outgoing(words.0);
        let size = message.size();
        message.0[1] = size;
        message
    }

    fn header(&mut self) -> *mut mach_msg_header_t {
        self.0.as_mut_ptr().cast()
    }

    fn size(&self) -> u32 {
        (self.0.len() * 4) as u32
    }
}

/// A message's words as they are written, four bytes each, which every part of it is a whole
/// number of.
#[derive(Default)]
struct Words(Vec<u32>);

impl Words {
    fn push(&mut self, word: u32) {
        self.0.push(word);
    }

    /// The header, its size filled in once the message is whole.
    fn header(&mut self, port: u32, protocol: u32, complex: bool) {
        let complex_bit = if complex { MACH_MSGH_BITS_COMPLEX } else { 0 };
        self.push(MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | complex_bit);
        self.push(0);
        self.push(port);
        self.push(MACH_PORT_NULL);
        self.push(MACH_PORT_NULL);
        self.push(protocol);
    }

    /// A send right moved to the receiver.
    fn port_descriptor(&mut self, name: u32) {
        self.push(name);
        self.push(0);
        let disposition = MACH_MSG_TYPE_MOVE_SEND << 16;
        let kind = MACH_MSG_PORT_DESCRIPTOR << 24;
        self.push(disposition | kind);
    }

    /// `bytes`, copied into the receiver's memory by the kernel, left the sender's.
    fn ool_descriptor(&mut self, bytes: &[u8]) {
        let address = bytes.as_ptr() as u64;
        self.push(address as u32);
        self.push((address >> 32) as u32);
        let copy = MACH_MSG_VIRTUAL_COPY << 8;
        let kind = MACH_MSG_OOL_DESCRIPTOR << 24;
        self.push(copy | kind);
        self.push(bytes.len() as u32);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            self.push(u32::from_ne_bytes(word));
        }
    }
}

/// Where a message is received into: aligned for a header, its size a whole number of words.
struct ReceiveBuffer(Vec<u64>);

impl ReceiveBuffer {
    fn sized(bytes: usize) -> ReceiveBuffer {
        ReceiveBuffer(vec![0; bytes.div_ceil(8)])
    }

    fn header(&mut self) -> *mut mach_msg_header_t {
        self.0.as_mut_ptr().cast()
    }

    fn capacity(&self) -> u32 {
        (self.0.len() * 8) as u32
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the buffer's words, seen as the bytes they are made of.
        unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast(), self.0.len() * 8) }
    }

    fn header_read(&self) -> mach_msg_header_t {
        // SAFETY: the buffer starts with a header the kernel wrote.
        unsafe { self.0.as_ptr().cast::<mach_msg_header_t>().read() }
    }

    fn word(&self, offset: usize) -> u32 {
        let bytes = self.bytes();
        u32::from_ne_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
    }

    /// The message the kernel wrote: a notification, or a message whose parts are taken out, of
    /// `protocol` or another one.
    fn read(&mut self, protocol: u32) -> io::Result<Arrival> {
        let header = self.header_read();
        if header.msgh_id == MACH_NOTIFY_NO_SENDERS {
            return Ok(Arrival::NoSenders);
        }
        let complex = header.msgh_bits & MACH_MSGH_BITS_COMPLEX != 0;
        let (ports, out_of_line, mut offset) = self.descriptors(complex);
        let inline_count = self.word(offset) as usize;
        offset += 4;
        let bytes = match out_of_line {
            Some(bytes) => bytes,
            None => self.bytes()[offset..offset + inline_count].to_vec(),
        };
        let trailer = (header.msgh_size as usize).next_multiple_of(4) + AUDIT_TOKEN_OFFSET;
        let sender = AuditToken(std::array::from_fn(|index| self.word(trailer + index * 4)));
        let received = Received {
            bytes,
            ports,
            sender,
        };
        let sent_in = header.msgh_id as u32;
        Ok(if sent_in == protocol {
            Arrival::Message(received)
        } else {
            Arrival::OtherProtocol {
                protocol: sent_in,
                received,
            }
        })
    }

    /// The ports a message carried and its out-of-line bytes, taken over, and where what follows
    /// the descriptors starts.
    fn descriptors(&self, complex: bool) -> (Vec<SendRight>, Option<Vec<u8>>, usize) {
        let mut ports = Vec::new();
        let mut out_of_line = None;
        if !complex {
            return (ports, out_of_line, HEADER_SIZE);
        }
        let count = self.word(HEADER_SIZE);
        let mut offset = HEADER_SIZE + BODY_SIZE;
        for _ in 0..count {
            let kind = u32::from(self.bytes()[offset + 11]);
            if kind == MACH_MSG_PORT_DESCRIPTOR {
                ports.push(SendRight::received(self.word(offset)));
                offset += PORT_DESCRIPTOR_SIZE;
            } else {
                out_of_line = Some(self.out_of_line(offset));
                offset += OOL_DESCRIPTOR_SIZE;
            }
        }
        (ports, out_of_line, offset)
    }

    /// The bytes of the out-of-line descriptor at `offset`, copied, and their pages let go.
    fn out_of_line(&self, offset: usize) -> Vec<u8> {
        let address = u64::from(self.word(offset)) | u64::from(self.word(offset + 4)) << 32;
        let size = self.word(offset + 12) as usize;
        if size == 0 {
            return Vec::new();
        }
        // SAFETY: the kernel mapped `size` bytes at `address` into this process for the message.
        let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, size) }.to_vec();
        // SAFETY: the pages are this process's since the message arrived, and nothing else
        // points into them.
        unsafe { mach_vm_deallocate(mach_task_self(), address, size as u64) };
        bytes
    }
}

#[cfg(test)]
mod tests;
