use crate::error::{ApplicationError, ProtoError};

use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use failure::Fail;
use ring::aead::{self, OpeningKey, SealingKey};
use serde::Serialize;
use std::io::{self, Cursor, Read, Write};
use std::mem;
use std::net::{SocketAddr, ToSocketAddrs};
use udt::{SocketFamily, SocketType, UdtSocket};

pub const BLOCK_SIZE: usize = 128 * 1024;
pub const MAGIC_BYTES: u32 = 0xDEADBEEF;
pub const MESSAGE_SIZE: usize = 12;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
enum MessageTy {
	/// The data which follows is an incoming block of data from the sender.
	/// It is encrypted with the parameters agreed upon at the beginning of
	/// the session.
	Block,

	/// The sender is informing the receiver that it would like initialization
	/// parameters for the session's encryption. The sender will wait for four
	/// bytes (32-bits) which will be prepended to a 64-bit counter for each 
	/// message sent.
	ReqIV,

	/// The receiver chooses encryption parameters for the session and sends
	/// them as the following four bytes.
	RepIV,

	/// The sender acknowledges receipt of the nonce with an encrypted `Hello`.
	Hello,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
	ty: MessageTy,
	len: usize
}

enum Mode {
	Sender,
	Receiver,
}

enum State {
	WaitHangup,
	WaitHello,
	Transmit,
}

struct Stream {
	inner: UdtSocket,
}

/// The `Stream` represents an underlying UDT socket.
impl Stream {
	/// When created in the `Receiver` mode it begins listening on the
	/// specified address. Otherwise if created in `Sender` mode it attempts
	/// to reach a receiver at the specified remote address.
	pub fn new<S: ToSocketAddrs>(mode: Mode, addr: S) -> Result<Self, ProtoError> {
		let sock_addr = addr.to_socket_addrs()?
			.take(1).next()
			.expect("fatal: expected a socket address but did not get one.");

		let stream = match mode {
			Mode::Sender => Self::create_sender(sock_addr)?,
			Mode::Receiver => Self::create_receiver(sock_addr)?,
		};

		Ok(stream)
	}


	fn create_sender(addr: SocketAddr) -> Result<Self, ProtoError> {
		info!("connecting to utp receiver ...");
		let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.connect(addr)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		Ok(Self { inner: sock })
	}

	fn create_receiver(addr: SocketAddr) -> Result<Self, ProtoError> {
		info!("setting up receiver socket ...");
		let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.bind(addr)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.listen(1)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		Ok(Self { inner: sock })
	}

	fn as_socket(&self) -> &UdtSocket { &self.inner }
}

impl Read for Stream {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
		let buf_len = buf.len();
		let bytes_recvd = self.inner.recv(buf, buf_len)
			.map_err(|err| ApplicationError::SocketErr { inner: err }.compat())
			.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

		// TODO: check the sanity of this cast.
		//       not sure why UDT has this as a signed integer.
		Ok(bytes_recvd as usize)
	}
}

impl Write for Stream {
	fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
		let bytes_sent = self.inner.send(&buf)
			.map_err(|err| ApplicationError::SocketErr { inner: err }.compat())
			.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

		// TODO: check the sanity of this cast.
		//       not sure why UDT has this as a signed integer.
		Ok(bytes_sent as usize)
	}

	fn flush(&mut self) -> Result<(), io::Error> {
		// TODO: UDT bindings provides no means to flush, I believe it's buffering
		// data internally and sending as fast as it can. (See: UDT_LINGER.)
		// for now this is a no-op since data is immediately committed to the
		// underlying UDT socket. 
		Ok(())
	}
}

/// A `Stream` implements both halves of the `ubuffer` protocol.
///
/// If a stream is created in the `Receiver` mode it will create a
/// UDT socket and begin listening on the specified address. When a
/// corresponding `Sender` connects to that same address the receiver
/// begins the handshake process which works as follows: 
///
/// 1. The receiver sends a `ReqIV` to the sender.
/// 2. The sender replies with a randomly created seed in a `RepIV` message.
/// 3. The receiver and sender both initialize their ciphers using their keys
///    (configured out of band) as well as the agreed upon IV.
/// 4. The receiver sends an encrypted `Hello` message with a nonce.
/// 5. The sender acknowledges this by encrypting its own `Hello` message
///    with a corresponding nonce.
/// 6. Both streams enter the `Transmit` state and begin exchanging encrypted
///    blocks with each other.
///
pub struct Sender {
	dec_key: OpeningKey,
	enc_key: SealingKey,

	stream: Stream,
	state: State,

	counter: u64,
	nonce:   u32,
}

impl Sender {
	pub fn new<S: ToSocketAddrs>(addr: S, key: &[u8]) -> Result<Self, ProtoError> {
		let stream = Stream::new(Mode::Sender, addr)?;
		let dec_key = OpeningKey::new(&aead::AES_256_GCM, key)?;
		let enc_key = SealingKey::new(&aead::AES_256_GCM, key)?;

		Ok(Self {
			dec_key: dec_key,
			enc_key: enc_key,

			stream: stream,
			state: State::WaitHello,

			counter: 0,
			nonce:   0,
		})
	}

	pub fn run<R: Read>(&mut self, mut input: R) -> Result<(), ProtoError> {
		info!("starting sender ...");

		loop {
			match self.state {
				State::WaitHangup => self.wait_hup()?,
				State::WaitHello => self.wait_hello()?,
				State::Transmit => self.transmit(&mut input)?,
			}
		}
	}

	fn wait_hup(&mut self) -> Result<(), ProtoError> {
		unreachable!()
	}

	fn wait_hello(&mut self) -> Result<(), ProtoError> {
		self.req_iv()?;
		self.recv_rep_iv()?;
		self.send_hello()?;
		self.recv_hello()?;

		info!("handshake complete ...");
		self.state = State::Transmit;

		Ok(())
	}

	fn req_iv(&mut self) -> Result<(), ProtoError> {
		// ask the server for the IV
		info!("sending IV request to remote peer ...");
		let req_iv_msg = Message {
			ty: MessageTy::ReqIV,
			len: 0,
		};

		let req_iv_buf = bincode::serialize(&req_iv_msg)
			.expect("fatal: could not serialize message");

		assert_eq!(MESSAGE_SIZE, req_iv_buf.len());
		self.stream.write(&req_iv_buf)?;

		Ok(())
	}

	fn recv_rep_iv(&mut self) -> Result<(), ProtoError> {
		// read the IV from the server
		info!("waiting for reply from server ...");
		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let rep_iv_msg: Message= bincode::deserialize(&buf)
			.expect("fatal: could not deserialize message");

		info!("got reply: {:?}", rep_iv_msg);
		let mut buf = vec![0u8; rep_iv_msg.len];
		self.stream.read_exact(&mut buf)?;

		let mut iv_cursor = Cursor::new(buf);
		self.nonce = iv_cursor.read_u32::<NetworkEndian>()?;
		info!("got iv: {:x}", self.nonce);

		Ok(())
	}

	fn send_hello(&mut self) -> Result<(), ProtoError> {
		info!("sending hello ...");

		// write the magic bytes to a buffer
		let tag_len = self.enc_key.algorithm().tag_len();
		let enc_buf = vec![0u8; mem::size_of_val(&MAGIC_BYTES) + tag_len];
		let mut enc_buf = {
			let mut cursor = Cursor::new(enc_buf);
			cursor.write_u32::<NetworkEndian>(MAGIC_BYTES)?;
			cursor.into_inner()
		};

		// encrypt the buffer in-place
		let msg_nonce = self.get_next_nonce()?;
		let msg_sz = aead::seal_in_place(&self.enc_key, &msg_nonce, b"", &mut enc_buf, tag_len)?;


		// send `Hello` followed by the encrypted payload
		let hello_msg = Message {
			ty: MessageTy::Hello,
			len: msg_sz,
		};

		let hello_buf = bincode::serialize(&hello_msg)
			.expect("fatal: could not serialize message");
		assert_eq!(hello_buf.len(), MESSAGE_SIZE);

		self.stream.write(&hello_buf)?;
		self.stream.write(&enc_buf[..msg_sz])?;

		Ok(())
	}

	fn recv_hello(&mut self) -> Result<(), ProtoError> {
		info!("receiving hello ...");

		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let hello_msg: Message= bincode::deserialize(&buf)
			.expect("fatal: could not deserialize message");

		if hello_msg.ty != MessageTy::Hello {
			return Err(ProtoError::UnexpectedMessage);
		}

		let mut buf = vec![0u8; hello_msg.len];
		let msg_nonce = self.get_next_nonce()?;
		self.stream.read_exact(&mut buf)?;
		let payload = aead::open_in_place(&self.dec_key, &msg_nonce, b"", 0, &mut buf)?;

		info!("decrypted hello of size: {}", payload.len());
		info!("hello was: {:?}", &payload);

		Ok(())
	}

	fn get_next_nonce(&mut self) -> Result<Box<[u8]>, ProtoError> {
		let buf = vec![0u8; 12];
		let mut cursor = Cursor::new(buf);

		let nonce = self.nonce;
		let counter = self.counter;
		self.counter += 1;
		
		cursor.write_u32::<NetworkEndian>(self.nonce)?;
		cursor.write_u64::<NetworkEndian>(self.counter)?;

		Ok(cursor.into_inner().into_boxed_slice())
	}

	fn transmit<R: Read>(&mut self, input: R) -> Result<(), ProtoError> {
		unreachable!()
	}

}

