ubuffer
=======

`ubuffer` is a simple file transfer program which is meant to facilitate
network transfers at extremely high throughput. Instead of using TCP (like
`scp`, `mbuffer`, `ftp`, et al.) we use a UDP based protocol (UDT[1]) instead.

NOTE: `ubuffer` *does not* encrypt data on the wire. Encryption must happen
at a higher level than this application. It is designed to work well over
UDP based VPNs, like WireGuard, which may provide sufficient security for
your use case.


## theory of operation

The `ubuffer` binary contains two primary modes: the sender (`-s`) and
the receiver (`-r`). The receiver binds to an IPv4 or IPv6 address and
awaits a single connection. The sender then connects to that same address
and begins streaming data to it.

The sender reads data from `stdin` to an internal buffer in 128KiB blocks.
The receiver copies all data, as it is received, to stdout.

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
