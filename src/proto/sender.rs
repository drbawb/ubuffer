use crate::error::ProtoError;
use crate::proto::{MessageTy, Message, Mode, State, Stream};
use crate::proto::{BLOCK_SIZE, MAGIC_BYTES, MESSAGE_SIZE};

use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use ring::aead::{self, OpeningKey, SealingKey};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::mem;
use std::net::ToSocketAddrs;

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

	/// This runs the `Sender` state machine to completion.
	/// 
	/// First the sender attempts to connect to the remote peer and
	/// perform a handshake. 
	///
	/// Once the encrypted channel is setup the sender begins reading
	/// chunks from stdin and encrypts them to be sent over the wire
	/// to the receiver.
	///
	/// Once the end of `stdin` has been reached the sender performs a
	/// closing handshake to attempt to cleanly shutdown the receiver
	/// and ensure that it has flushed all contents to its output buffer.
	pub fn run<R: Read>(&mut self, mut input: R) -> Result<(), ProtoError> {
		info!("starting sender ...");

		loop {
			match self.state {
				State::WaitHello => self.wait_hello()?,
				State::Transmit => self.transmit(&mut input)?,

				State::WaitHangup => {
					self.wait_hup()?;
					return Ok(());
				}
			}
		}
	}

	fn transmit<R: Read>(&mut self, input: R) -> Result<(), ProtoError> {
		let tag_len = self.enc_key.algorithm().tag_len();
		let mut reader = BufReader::with_capacity(BLOCK_SIZE, input);
		let mut enc_buffer = vec![0u8; BLOCK_SIZE + tag_len];

		'copy: loop {
			let chunk = reader.fill_buf()?;
			trace!("copying block from stdin {}", enc_buffer.len());
			trace!("block size: {}", chunk.len());
			let mut input_cursor = Cursor::new(&chunk);
			let mut enc_cursor = Cursor::new(&mut enc_buffer[..BLOCK_SIZE]);
			let bytes_read = io::copy(&mut input_cursor, &mut enc_cursor)? as usize;

			// TODO: why is io::copy returning a u64?
			trace!("copied {} bytes", bytes_read);
			reader.consume(bytes_read);

			if bytes_read == 0 {
				debug!("buffer reached eof");
				break 'copy;
			}

			trace!("encrypting block w/ tag {}", tag_len);
			assert!(bytes_read <= BLOCK_SIZE);
			let nonce = self.get_next_nonce()?;
			let enc_msg_len = bytes_read + tag_len;
			let enc_size = aead::seal_in_place(&self.enc_key, &nonce, b"", &mut enc_buffer[..enc_msg_len], tag_len)?;

			// create encrypted packet header
			let block_msg = Message {
				ty: MessageTy::Block,
				len: enc_size,
			};

			trace!("sending block message: {:?}", block_msg);
			let block_buf = bincode::serialize(&block_msg)?;
			assert_eq!(block_buf.len(), MESSAGE_SIZE);

			self.stream.write(&block_buf)?;

			let mut pos = 0;
			'write: loop {
				let bytes_sent = self.stream.write(&enc_buffer[pos..enc_size])?;
				pos += bytes_sent as usize;

				trace!("pos: {}, sent: {}, len: {}", pos, bytes_sent, bytes_read);
				if pos >= enc_size { break 'write; }
			}
		}

		self.state = State::WaitHangup;
		Ok(())
	}

	fn wait_hup(&mut self) -> Result<(), ProtoError> {
		self.send_client_goodbye()?;
		self.recv_server_goodbye()?;
		Ok(())
	}

	fn wait_hello(&mut self) -> Result<(), ProtoError> {
		self.req_iv()?;
		self.recv_rep_iv()?;
		self.send_hello()?;
		self.recv_hello()?;

		info!("handshake complete!");
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

		let req_iv_buf = bincode::serialize(&req_iv_msg)?;

		assert_eq!(MESSAGE_SIZE, req_iv_buf.len());
		self.stream.write(&req_iv_buf)?;

		Ok(())
	}

	fn recv_rep_iv(&mut self) -> Result<(), ProtoError> {
		// read the IV from the server
		info!("waiting for reply from server ...");
		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let rep_iv_msg: Message= bincode::deserialize(&buf)?;

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

		let hello_buf = bincode::serialize(&hello_msg)?;
		assert_eq!(hello_buf.len(), MESSAGE_SIZE);

		self.stream.write(&hello_buf)?;
		self.stream.write(&enc_buf[..msg_sz])?;

		Ok(())
	}
	
	fn send_client_goodbye(&mut self) -> Result<(), ProtoError> {
		let goodbye_msg = Message {
			ty: MessageTy::Goodbye,
			len: 0,
		};

		let goodbye_buf = bincode::serialize(&goodbye_msg)?;
		self.stream.write(&goodbye_buf)?;

		Ok(())
	}

	fn recv_hello(&mut self) -> Result<(), ProtoError> {
		info!("receiving hello ...");

		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let hello_msg: Message= bincode::deserialize(&buf)?;

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

	fn recv_server_goodbye(&mut self) -> Result<(), ProtoError> {
		info!("receiving goodbye ...");

		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let goodbye_msg: Message = bincode::deserialize(&buf)?;

		if goodbye_msg.ty != MessageTy::Goodbye {
			return Err(ProtoError::UnexpectedMessage);
		}

		info!("goodbye world ...");
		Ok(())
	}

	fn get_next_nonce(&mut self) -> Result<Box<[u8]>, ProtoError> {
		let buf = vec![0u8; 12];
		let mut cursor = Cursor::new(buf);

		self.counter += 1;
		
		cursor.write_u32::<NetworkEndian>(self.nonce)?;
		cursor.write_u64::<NetworkEndian>(self.counter)?;

		Ok(cursor.into_inner().into_boxed_slice())
	}
}
