ubuffer
=======

`ubuffer` is a simple file transfer program which is meant to facilitate
network transfers at extremely high throughput. Instead of using TCP (like
`scp`, `mbuffer`, `ftp`, et al.) we use a UDP based protocol (UDT[1]) instead.

NOTE: `ubuffer` is *alpha quality* software. It has not undergone any sort
of analysis as to its performance, the soundness of its encryption implementation,
etc. Use it strictly at your own risk.

## building

This is a Rust project which can be built using the Cargo package manager.
Install Rust using your OS distribution's preferred package manager, or visit
[the Rust website](https://www.rust-lang.org/) for further instructions.

Once you have a working installation of `rustc` and `cargo` on your `PATH` you
can build this project using the following steps:

1. `git clone https://github.com/drbawb/ubuffer` to download the source fiels
2. `cd ubuffer` to enter the directory w/ the Cargo.toml file
3. `cargo build --release` to build an optimized copy of the software

The finished binary will be placed in `target/release/ubuffer` which can be
installed on your PATH using your preferred method.

## usage

The `ubuffer help` command will print usage instructions. You can use
`ubuffer help <subcommand>` to get more detailed information about a 
specific command.

```
USAGE:
    ubuffer receiver <LISTEN_ADDR> --key <KEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -k, --key <KEY>    The encryption key used to encrypt data blocks. (Must match on both sender & receiver.)

ARGS:
    <LISTEN_ADDR>    The address and port to listen on for incoming senders.
```

An example `ubuffer` session might look something like this:

1. `ubuffer genkey` will print a base64 encoded encryption key, you
   will need to copy this as it will be needed to start both the sender
   and receiver.

2. `ubuffer receiver [address] -k [key] > output.txt` will start the
   program in "receiver mode" bound to the specified address and port.
   it will use the specified key to decrypt incoming data blocks.

3. `cat input.txt | ubufer sender [address] -k [key]` will start the
   program in "sender mode", it will copy the data from stdin and encrypt
   it using the specified key. the data will be sent to the receiver at the
   specified address.

## theory of operation

The `ubuffer` program operates in two primary modes: the receiver, which
listens for data on a UDP socket, and the sender which transmits data to
that remote socket.

The receiver waits to accept one, and only one, incoming client connection.
If a client connects and fails to properly handshake the receiver will
terminate. At present the receiver *does not* support multiple clients in
any way.

The client connects to a receiver and performs a simple handshake. It sends
an unencrypted message asking the receiver to generate a nonce for the session.
The receiver replies with the nonce, similarly in the clear. Once both sides 
have the nonce the client encrypts a `Hello` message and sends it to the receiver.
If the receiver is able to successfully decrypt this message it likewise encrypts
a `Hello` and sends it to the sender.

Once the sender & receiver have exchanged this encrypted handshake the sender is
free to begin transmitting encrypted data blocks. To do so it first sends a fixed
header indicating the size of the encrypted payload, each time such a header is
received internal counters are incremented on both sides of the connection.

The receiver reads the length specified and attempts to decrypt the packet. If at
any time decryption fails the receiver tears down the connection immediately. Once
the sender has finished sending blocks it sends an (unencrypted) `Goodbye` header. 
The receiver upon reading a `Goodbye` header acknowledges receipt of it, at which
point the sender tears down the connection gracefully and the server exits.

## future improvements

- Potentially look at how much data is being sent by UDT per exchange,
  and dynamically size our internal buffers accordingly?

- Allow the buffers to be configured via parameters?

- Allow multiple blocks to be buffered at once to prevent blocking
  the sender if it is copying from a latent source? (i.e: tape, spinning 
  rust, etc.)

- Display measurements on stderr?

- Higher level protocol functionality?
  - built-in encryption? (TLS?)
  - handshakes at beginning/end instead of just closing the socket?
  - authenticated sender/receivers?

[1]: http://udt.sourceforge.net/ 
