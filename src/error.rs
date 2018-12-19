use std::convert::From;

#[derive(Fail, Debug)]
pub enum ApplicationError {
	#[fail(display = "could not parse the specified network address")]
	MissingAddr,

	#[fail(display = "failed to connect to socket")]
	SocketErr { inner: udt::UdtError },

	#[fail(display = "failed to send data through socket")]
	TxErr { inner: udt::UdtError },

	#[fail(display = "failed to receive data through socket")]
	RxErr { inner: udt::UdtError },
}

#[derive(Fail, Debug)]
pub enum ProtoError {
	#[fail(display = "unexpected crypto error")]
	CryptoErr,

	#[fail(display = "unexpected i/o error")]
	IoErr { inner: std::io::Error },

	#[fail(display = "serialization failure")]
	SerializeErr { inner: bincode::Error },

	#[fail(display = "unexpected network socket error")]
	SocketErr { inner: udt::UdtError },

	#[fail(display = "message type was not expected at this time ...")]
	UnexpectedMessage,
}

impl From<ring::error::Unspecified> for ProtoError {
	fn from(err: ring::error::Unspecified) -> Self {
		ProtoError::CryptoErr
	}
}

impl From<std::io::Error> for ProtoError {
	fn from(err: std::io::Error) -> Self {
		ProtoError::IoErr { inner: err }
	}
}

impl From<udt::UdtError> for ProtoError {
	fn from(err: udt::UdtError) -> Self {
		ProtoError::SocketErr { inner: err }
	}
}

impl From<bincode::Error> for ProtoError {
	fn from(err: bincode::Error) -> Self {
		ProtoError::SerializeErr { inner: err }
	}
}
