extern crate mio;
extern crate sha1;
extern crate rustc_serialize;
extern crate http_muncher;
extern crate byteorder;

use mio::{Token,PollOpt,Ready};
use mio::deprecated::{EventLoop, Handler};
use std::net::SocketAddr;
use mio::tcp::{TcpListener, TcpStream};
use std::collections::HashMap;
use http_muncher::{Parser, ParserHandler};
use std::io::{Read, Write};
use rustc_serialize::base64::{ToBase64, STANDARD};
use std::cell::RefCell;
use std::rc::Rc;
use std::fmt;
mod frame;
use frame::WebSocketFrame;

fn gen_key(key: &String) -> String {
	let mut m = sha1::Sha1::new();
	let mut buf = [0u8; 20];
	m.update(key.as_bytes());
	m.update("258EAFA5-E914-47DA-95CA-C5AB0DC85B11".as_bytes());
	m.output(&mut buf);
	return buf.to_base64(STANDARD);
}

struct HttpParser {
	current_key: Option<String>,
	headers: Rc<RefCell<HashMap<String, String>>>
}

enum ClientState {
	AwaitingHandshake(RefCell<Parser<HttpParser>>),
	HandshakeResponse,
	Connected
}

impl ParserHandler for HttpParser {
	fn on_header_field(&mut self, s: &[u8]) -> bool {
		self.current_key = Some(std::str::from_utf8(s).unwrap().to_string());
		true
	}

	fn on_header_value(&mut self, s: &[u8]) -> bool {
		self.headers.borrow_mut().insert(self.current_key.clone().unwrap(),
			std::str::from_utf8(s).unwrap().to_string());
		true
	}

	fn on_headers_complete(&mut self) -> bool {
		false
	}
}

struct WebSocketClient {
	socket: TcpStream,
	headers: Rc<RefCell<HashMap<String, String>>>,
	interest: Ready,
	state: ClientState,
	outgoing: Vec<WebSocketFrame>
}

impl WebSocketClient {
	fn read(&mut self) {
		match self.state {
			ClientState::AwaitingHandshake(_) => {
				self.read_handshake();
			},
			ClientState::Connected => {
				let frame = WebSocketFrame::read(&mut self.socket);
				match frame {
					Ok(frame) => {
						println!("{:?}", frame);

						let reply_frame = WebSocketFrame::from("Hi there!");
						self.outgoing.push(reply_frame);

						if self.outgoing.len() > 0 {
							self.interest.remove(Ready::readable());
							self.interest.insert(Ready::writable());
						}
					}
					Err(e) => println!("error while reading frame: {}", e)
				}
			}
			_ => {}
		}
	}

	fn read_handshake(&mut self) {
		loop {
			let mut buf = [0;2048];
			match self.socket.read(&mut buf) {
				Err(e) => {
					println!("Error while reading socket {:?}", e);
					return
				},
				Ok(len) => {
					let is_upgrade = if let ClientState::AwaitingHandshake(ref parser_state) = self.state {
						let mut parser = parser_state.borrow_mut();
						parser.parse(&buf[0..len]);
						parser.is_upgrade()
					} else {false};
					if is_upgrade {
						println!("Got upgrade");
						self.state = ClientState::HandshakeResponse;
						self.interest.remove(Ready::readable());
						self.interest.insert(Ready::writable());
						break
					}
				}
			}
		}
	}

	fn write(&mut self) {
		match self.state {
			ClientState::HandshakeResponse => self.write_handshake(),
			ClientState::Connected => {
				println!("Sending {} frames", outgoing.len());

				for frame in self.outgoing.iter() {
					if let Err(e) = frame.write(&mut self.socket) {
						println!("error on write: {}", e);
					}
				}

				self.outgoing.clear();

				self.interest.remove(Ready::writable());
				self.interest.insert(Ready::readable());
			},
			_ => {}
		}
	}

	fn write_handshake(&mut self) {
		let headers = self.headers.borrow();
		let response_key = gen_key(&headers.get("Sec-WebSocket-Key").unwrap());
		let response = fmt::format(format_args!("HTTP/1.1 101 Switching Protocols\r\n\
                                                 Connection: Upgrade\r\n\
                                                 Sec-WebSocket-Accept: {}\r\n\
                                                 Upgrade: websocket\r\n\r\n", response_key));
		self.socket.write(response.as_bytes()).unwrap();
		self.state = ClientState::Connected;
		self.interest.remove(Ready::writable());
		self.interest.insert(Ready::readable());
	}

	fn new(socket: TcpStream) -> WebSocketClient {
		let headers = Rc::new(RefCell::new(HashMap::new()));
		WebSocketClient {
			socket: socket,
			headers: headers.clone(),
			interest: Ready::readable(),
			state: ClientState::AwaitingHandshake(RefCell::new(Parser::request(HttpParser {
				current_key: None,
				headers: headers.clone()
			}))),
			outgoing: Vec::new()
		}
	}
}

struct WebSocketServer {
	socket: TcpListener,
	clients: HashMap<Token, WebSocketClient>,
	token_counter: usize
}

const SERVER_TOKEN: Token = Token(0);


impl Handler for WebSocketServer {
	type Timeout = usize;
	type Message = ();

	fn ready(&mut self, event_loop: &mut EventLoop<WebSocketServer>,
		token: Token, events: Ready) {
		if events.is_readable() {
			match token {
				SERVER_TOKEN => {
					let client_socket = match self.socket.accept() {
						Err(e) => {
							println!("Accept error: {}", e);
							return;
						},
						Ok((sock, addr)) => sock
					};

					self.token_counter += 1;
					let new_token = Token(self.token_counter);
					self.clients.insert(new_token, WebSocketClient::new(client_socket));
					event_loop.register(&self.clients[&new_token].socket, new_token, Ready::readable(),
						PollOpt::edge() | PollOpt::oneshot()).unwrap();
				},
				token => {
					let mut client = self.clients.get_mut(&token).unwrap();
					client.read();
					event_loop.reregister(&client.socket, token, client.interest,
						PollOpt::edge() | PollOpt::oneshot()).unwrap();
				}
			}
		}
		if events.is_writable() {
			let mut client = self.clients.get_mut(&token).unwrap();
			client.write();
			event_loop.reregister(&client.socket, token, client.interest,
				PollOpt::edge() | PollOpt::oneshot()).unwrap();
		}
	}
}

fn main() {
	let address = "127.0.0.1:10000".parse::<SocketAddr>().unwrap();
	let server_socket = TcpListener::bind(&address).unwrap();
	let mut server = WebSocketServer {
		socket: server_socket,
		clients: HashMap::new(),
		token_counter: 0
	};
    let mut event_loop = EventLoop::new().unwrap();
    event_loop.register(&server.socket, Token(0), Ready::readable(), PollOpt::edge()).unwrap();
    event_loop.run(&mut server).unwrap();
}
