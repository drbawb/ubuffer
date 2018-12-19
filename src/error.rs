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
	#[fail(display = "unexpected i/o error")]
	IoErr { inner: std::io::Error },


	#[fail(display = "unexpected network socket error")]
	SocketErr { inner: udt::UdtError },
}

impl From<std::io::Error> for ProtoError {
	fn from(err: std::io::Error) -> Self {
		ProtoError::IoErr { inner: err }
	}
}
