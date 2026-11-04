//! Support of the Non/New Session Manager (NSM) protocol.
//!
//! When a program is started by a session manager, the environment variable NSM_URL
//! contains the address of the server. The program then:
//! 1. announces itself to the server and waits to be told where its files are and how
//!    its JACK client must be named,
//! 2. restores the state that was saved for it in the session,
//! 3. saves its state whenever the server asks for it,
//! 4. quits when it receives the SIGTERM signal.
//!
//! Without NSM_URL, the program behaves as if no session manager existed: nothing is
//! saved and the JACK client keeps its usual name.
//!
//! The state of a program lives in its real time thread. To save it without blocking
//! that thread, the process handler is wrapped in a 'SessionHandler' that copies the
//! state after each cycle into a snapshot, only if the snapshot is not being saved.

mod osc;
mod state;

use std::{
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    time::Duration,
};

use osc::{Arg, Message};
pub use state::{sync, ConfigurationState, Persistent, Stateful, Values};

use crate::utils::CommonError;

///The environment variable containing the address of the session manager
const NSM_URL: &str = "NSM_URL";
///The version of the protocol that is implemented
const API_MAJOR: i32 = 1;
const API_MINOR: i32 = 2;
///The optional features of the protocol supported by the programs. None of them is.
const CAPABILITIES: &str = ":";
///The name of the file containing the state, in the directory given by the session manager
const STATE_FILE: &str = "state.conf";
///The error code for a generic error
const ERR_GENERAL: i32 = -1;
///The maximum time to wait for an answer from the server when starting
const SERVER_TIMEOUT: Duration = Duration::from_secs(10);
///How often the signals are checked
const POLL_INTERVAL: Duration = Duration::from_millis(100);
///How long the user interface has to close itself before the program is stopped
const QUIT_TIMEOUT: Duration = Duration::from_secs(5);

const ANNOUNCE: &str = "/nsm/server/announce";
const OPEN: &str = "/nsm/client/open";
const SAVE: &str = "/nsm/client/save";

///True if the program is managed by a session manager
static MANAGED: AtomicBool = AtomicBool::new(false);
///True once the program was asked to quit
static TERMINATE: AtomicBool = AtomicBool::new(false);
///The context of the user interface, used to close it when the program has to quit
static UI_CONTEXT: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

///Return true if the program is started by a session manager
pub fn is_session_managed() -> bool {
    MANAGED.load(Ordering::Relaxed)
}

///Specify the user interface to close when the session manager asks the program to quit
pub fn set_ui_context(ctx: &eframe::egui::Context) {
    *UI_CONTEXT.lock().unwrap_or_else(PoisonError::into_inner) = Some(ctx.clone());
}

///Block until the session manager asks the program to quit. Meant for the
/// programs without user interface
pub fn wait_for_quit() {
    while !TERMINATE.load(Ordering::Relaxed) {
        std::thread::sleep(POLL_INTERVAL);
    }
}

extern "C" fn on_terminate(_signal: libc::c_int) {
    TERMINATE.store(true, Ordering::Relaxed);
}

///Close the user interface. If this does not stop the program in time, it is stopped
fn quit() {
    if let Some(ctx) = UI_CONTEXT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
    std::thread::sleep(QUIT_TIMEOUT);
    eprintln!("The program did not stop by itself: exiting");
    std::process::exit(0);
}

///Retrieve the address of the server from the value of NSM_URL (osc.udp://host:port/)
fn server_address(url: &str) -> Option<SocketAddr> {
    let host_port = url.strip_prefix("osc.udp://")?.split('/').next()?;
    let addresses: Vec<SocketAddr> = host_port.to_socket_addrs().ok()?.collect();
    //the server usually listens on IPv4 only
    addresses
        .iter()
        .find(|a| a.is_ipv4())
        .or(addresses.first())
        .copied()
}

///The name of the executable, that the session manager uses to restart the program
fn executable_name(default: &str) -> String {
    std::env::args()
        .next()
        .and_then(|arg| {
            Path::new(&arg)
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| String::from(default))
}

///The link with the session manager
struct Connection {
    socket: UdpSocket,
    server: SocketAddr,
    ///The directory in which the files of the program are stored
    directory: PathBuf,
    ///The unique id given to the program by the session manager. It is used as JACK client name
    client_id: String,
    ///The name under which the program is known by the session manager
    app_name: String,
}

impl Connection {
    fn send(&self, msg: Message) {
        if let Err(e) = self.socket.send_to(&msg.encode(), self.server) {
            eprintln!("Unable to send {} to the session manager: {e}", msg.address);
        }
    }

    ///Tell the server that a request succeeded
    fn reply(&self, request: &str, message: &str) {
        self.send(Message::new(
            "/reply",
            vec![Arg::from(request), Arg::from(message)],
        ));
    }

    ///Tell the server that a request failed
    fn error(&self, request: &str, message: &str) {
        self.send(Message::new(
            "/error",
            vec![
                Arg::from(request),
                Arg::from(ERR_GENERAL),
                Arg::from(message),
            ],
        ));
    }

    ///Wait for the next message. None is returned if the packet is not a valid message
    fn receive(&self) -> std::io::Result<Option<Message>> {
        let mut buffer = [0; 4096];
        let (size, _) = self.socket.recv_from(&mut buffer)?;
        Ok(Message::decode(&buffer[..size]))
    }

    fn state_file(&self) -> PathBuf {
        self.directory.join(STATE_FILE)
    }

    fn save(&self, values: &Values) -> std::io::Result<()> {
        //the previous state is only replaced once the new one is completely written
        let tmp = self.directory.join(format!("{STATE_FILE}.tmp"));
        std::fs::write(&tmp, values.to_text())?;
        std::fs::rename(&tmp, self.state_file())
    }

    ///Answer the requests of the server until the program has to quit
    fn run(self, state: impl Fn() -> Values) {
        if let Err(e) = self.socket.set_read_timeout(Some(POLL_INTERVAL)) {
            eprintln!("Unable to configure the session manager socket: {e}");
        }
        loop {
            if TERMINATE.load(Ordering::Relaxed) {
                quit();
            }
            match self.receive() {
                Ok(Some(msg)) => self.handle(&msg, &state),
                Ok(None) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => {
                    eprintln!("Error while listening to the session manager: {e}");
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }

    ///Answer a request of the server
    fn handle(&self, msg: &Message, state: &impl Fn() -> Values) {
        //other messages are only informative
        if msg.address == SAVE {
            match self.save(&state()) {
                Ok(()) => self.reply(SAVE, "Saved"),
                Err(e) => self.error(SAVE, &format!("Unable to save the state: {e}")),
            }
        }
    }
}

///The real time processing unit, saving its state after each cycle
pub struct SessionHandler<H: Stateful> {
    handler: H,
    snapshot: Arc<Mutex<H::State>>,
}

impl<H: jack::ProcessHandler + Stateful> jack::ProcessHandler for SessionHandler<H> {
    fn process(&mut self, client: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let control = self.handler.process(client, ps);
        //never wait: if the state is being saved, it will be updated on the next cycle
        if let Ok(mut state) = self.snapshot.try_lock() {
            self.handler.publish(&mut state);
        }
        control
    }

    fn buffer_size(&mut self, client: &jack::Client, size: jack::Frames) -> jack::Control {
        self.handler.buffer_size(client, size)
    }
}

///The session in which the program runs, if any
pub struct Session {
    connection: Option<Connection>,
}

impl Session {
    ///Join the session manager if the program was started by one. This waits
    /// until the server tells where the files of the program are.
    pub fn start(app_name: &str) -> Result<Session, CommonError> {
        match std::env::var(NSM_URL) {
            Ok(url) if !url.is_empty() => Self::connect(app_name, &url),
            _ => Ok(Session { connection: None }),
        }
    }

    ///Join the session manager listening at the given url
    fn connect(app_name: &str, url: &str) -> Result<Session, CommonError> {
        let error = |msg: String| CommonError::SessionError(msg);

        let server =
            server_address(&url).ok_or_else(|| error(format!("Invalid {NSM_URL}: {url}")))?;
        let local = if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket =
            UdpSocket::bind(local).map_err(|e| error(format!("Unable to open a socket: {e}")))?;
        socket
            .set_read_timeout(Some(SERVER_TIMEOUT))
            .map_err(|e| error(format!("Unable to configure the socket: {e}")))?;
        let mut connection = Connection {
            socket,
            server,
            directory: PathBuf::new(),
            client_id: String::new(),
            app_name: String::from(app_name),
        };

        connection.send(Message::new(
            ANNOUNCE,
            vec![
                Arg::from(app_name),
                Arg::from(CAPABILITIES),
                Arg::from(executable_name(app_name).as_str()),
                Arg::from(API_MAJOR),
                Arg::from(API_MINOR),
                Arg::from(std::process::id() as i32),
            ],
        ));

        //the server answers the announce, then asks the program to open its session
        loop {
            let msg = match connection.receive() {
                Ok(Some(msg)) => msg,
                Ok(None) => continue,
                Err(e) => return Err(error(format!("No answer from the session manager: {e}"))),
            };
            match msg.address.as_str() {
                "/error" if msg.str_arg(0) == Some(ANNOUNCE) => {
                    return Err(error(format!(
                        "Refused by the session manager: {}",
                        msg.str_arg(2).unwrap_or("unknown reason")
                    )))
                }
                OPEN => {
                    if let (Some(path), Some(client_id)) = (msg.str_arg(0), msg.str_arg(2)) {
                        connection.directory = PathBuf::from(path);
                        connection.client_id = String::from(client_id);
                        break;
                    }
                }
                _ => {}
            }
        }

        MANAGED.store(true, Ordering::Relaxed);
        //from here, any error is reported to the server when the session is dropped
        let session = Session {
            connection: Some(connection),
        };
        if let Some(directory) = session.directory() {
            std::fs::create_dir_all(directory).map_err(|e| {
                error(format!(
                    "Unable to create the directory {}: {e}",
                    directory.display()
                ))
            })?;
        }
        Ok(session)
    }

    ///The directory where the program has to store its files, if it is managed
    pub fn directory(&self) -> Option<&Path> {
        self.connection.as_ref().map(|c| c.directory.as_path())
    }

    ///The title of the program's window: 'client name - client id' if the program
    /// is managed by a session manager, 'default' otherwise
    pub fn window_title(&self, default: &str) -> String {
        match &self.connection {
            Some(c) => format!("{} - {}", c.app_name, c.client_id),
            None => String::from(default),
        }
    }

    ///Create the JACK client. Its name is chosen by the session manager, if any
    pub fn client(&self, default_name: &str) -> Result<jack::Client, CommonError> {
        let (name, options) = match &self.connection {
            Some(c) => (
                c.client_id.as_str(),
                jack::ClientOptions::NO_START_SERVER | jack::ClientOptions::USE_EXACT_NAME,
            ),
            None => (default_name, jack::ClientOptions::NO_START_SERVER),
        };
        match jack::Client::new(name, options) {
            Ok((client, _status)) => Ok(client),
            Err(e) => Err(CommonError::UnableToStartClient(e)),
        }
    }

    ///Restore the state saved in the session into the given state
    pub fn load<T: Persistent>(&self, mut state: T) -> T {
        if let Some(connection) = &self.connection {
            match std::fs::read_to_string(connection.state_file()) {
                Ok(text) => state.load(&Values::from_text(&text)),
                //nothing was saved yet
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => eprintln!("Unable to read the saved state: {e}"),
            }
        }
        state
    }

    ///Activate the client. From now on, the state of the handler is saved on request.
    /// 'state' must be the state the handler starts with
    pub fn activate<H>(
        mut self,
        client: jack::Client,
        handler: H,
        state: H::State,
    ) -> Result<jack::AsyncClient<(), SessionHandler<H>>, CommonError>
    where
        H: jack::ProcessHandler + Stateful + 'static,
    {
        let snapshot = Arc::new(Mutex::new(state));
        let handler = SessionHandler {
            handler,
            snapshot: snapshot.clone(),
        };
        let active_client = match client.activate_async((), handler) {
            Ok(client) => client,
            Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
        };
        self.serve(move || {
            let mut values = Values::new();
            snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .save(&mut values);
            values
        });
        Ok(active_client)
    }

    ///Activate a client that has nothing to save
    pub fn activate_stateless<H>(
        mut self,
        client: jack::Client,
        handler: H,
    ) -> Result<jack::AsyncClient<(), H>, CommonError>
    where
        H: jack::ProcessHandler + 'static,
    {
        let active_client = match client.activate_async((), handler) {
            Ok(client) => client,
            Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
        };
        self.serve(Values::new);
        Ok(active_client)
    }

    ///Tell the server that the program is ready and answer its requests
    fn serve(&mut self, state: impl Fn() -> Values + Send + 'static) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        unsafe {
            libc::signal(
                libc::SIGTERM,
                on_terminate as *const () as libc::sighandler_t,
            );
        }
        connection.reply(OPEN, "Ready");
        std::thread::spawn(move || connection.run(state));
    }
}

impl Drop for Session {
    ///If the program could not start, the server is told so
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            connection.error(OPEN, "Unable to start the program");
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        net::{SocketAddr, UdpSocket},
        time::Duration,
    };

    use super::{
        osc::{Arg, Message},
        server_address, Persistent, Session, Values, ANNOUNCE, OPEN, SAVE,
    };
    use crate::synth::hardware::{HardWare, KeyBoardKey};

    fn receive(server: &UdpSocket) -> (Message, SocketAddr) {
        let mut buffer = [0; 4096];
        let (size, from) = server.recv_from(&mut buffer).unwrap();
        (Message::decode(&buffer[..size]).unwrap(), from)
    }

    fn fake_server() -> (UdpSocket, String) {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let url = format!("osc.udp://{}/", server.local_addr().unwrap());
        (server, url)
    }

    #[test]
    fn session_protocol() {
        let (server, url) = fake_server();
        let directory =
            std::env::temp_dir().join(format!("rsuite-nsm-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);

        let dir = directory.clone();
        let server = std::thread::spawn(move || {
            let (announce, client) = receive(&server);
            assert_eq!(announce.address, ANNOUNCE);
            assert_eq!(announce.str_arg(0), Some("Test"));
            assert_eq!(announce.args[3], Arg::Int(1));
            let reply = Message::new(
                "/reply",
                vec![
                    Arg::from(ANNOUNCE),
                    Arg::from("Welcome"),
                    Arg::from("server"),
                    Arg::from(":"),
                ],
            );
            server.send_to(&reply.encode(), client).unwrap();
            let open = Message::new(
                OPEN,
                vec![
                    Arg::from(dir.to_str().unwrap()),
                    Arg::from("Test"),
                    Arg::from("nTEST"),
                ],
            );
            server.send_to(&open.encode(), client).unwrap();
            server
        });
        let session = Session::connect("Test", &url).unwrap();
        let server = server.join().unwrap();
        assert!(directory.is_dir());
        assert_eq!(session.connection.as_ref().unwrap().client_id, "nTEST");
        assert_eq!(session.window_title("Default"), "Test - nTEST");
        assert_eq!(Session { connection: None }.window_title("Test"), "Test");

        //nothing was saved: the given state is kept
        assert_eq!(session.load(HardWare::default()), HardWare::default());

        let mut keyboard = HardWare::new();
        keyboard.update_key(KeyBoardKey::Play, 3);
        let saved = keyboard.clone();
        session
            .connection
            .as_ref()
            .unwrap()
            .handle(&Message::new(SAVE, Vec::new()), &move || {
                let mut values = Values::new();
                saved.save(&mut values);
                values
            });
        let (reply, _) = receive(&server);
        assert_eq!(
            reply,
            Message::new("/reply", vec![Arg::from(SAVE), Arg::from("Saved")])
        );
        assert_eq!(session.load(HardWare::default()), keyboard);

        //the session was not activated: the server is told that the program failed
        drop(session);
        let (error, _) = receive(&server);
        assert_eq!(error.address, "/error");
        assert_eq!(error.str_arg(0), Some(OPEN));

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn refused_announce() {
        let (server, url) = fake_server();
        let server = std::thread::spawn(move || {
            let (_, client) = receive(&server);
            let error = Message::new(
                "/error",
                vec![Arg::from(ANNOUNCE), Arg::Int(-2), Arg::from("Bad API")],
            );
            server.send_to(&error.encode(), client).unwrap();
        });
        assert!(Session::connect("Test", &url).is_err());
        server.join().unwrap();
    }

    #[test]
    fn parse_server_address() {
        let addr = server_address("osc.udp://127.0.0.1:15432/").unwrap();
        assert_eq!(addr, "127.0.0.1:15432".parse().unwrap());
        let addr = server_address("osc.udp://localhost:15432/").unwrap();
        assert_eq!(addr.port(), 15432);
        assert_eq!(server_address("osc.tcp://127.0.0.1:15432/"), None);
        assert_eq!(server_address("osc.udp://127.0.0.1/"), None);
    }
}
