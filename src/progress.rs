use std::{
    env,
    io::{self, IsTerminal, Write},
    sync::{
        Mutex, MutexGuard,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DRAW_INTERVAL: Duration = Duration::from_millis(100);
const INITIAL_DELAY: Duration = Duration::from_millis(150);
static STDERR_STATE: Mutex<bool> = Mutex::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Phase(String),
    Suspend,
}

pub type Reporter<'a> = &'a dyn Fn(Event);

pub fn none(_: Event) {}

pub fn phase(reporter: Reporter<'_>, message: impl Into<String>) {
    reporter(Event::Phase(message.into()));
}

pub struct Spinner {
    sender: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

pub struct StderrWriter {
    _guard: MutexGuard<'static, bool>,
    stderr: io::Stderr,
    clear: bool,
}

pub fn stderr_writer() -> StderrWriter {
    let mut guard = lock_stderr();
    let clear = std::mem::take(&mut *guard);
    StderrWriter {
        _guard: guard,
        stderr: io::stderr(),
        clear,
    }
}

impl Write for StderrWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.clear {
            self.stderr.write_all(b"\r\x1b[2K")?;
            self.clear = false;
        }
        self.stderr.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()
    }
}

enum Command {
    Event(Event),
    Suspend(Sender<()>),
}

impl Spinner {
    pub fn new(enabled: bool) -> Self {
        if !should_start(
            enabled,
            io::stderr().is_terminal(),
            env::var("TERM").ok().as_deref(),
        ) {
            return Self {
                sender: None,
                thread: None,
            };
        }
        Self::start(io::stderr(), INITIAL_DELAY)
    }

    fn start(mut output: impl Write + Send + 'static, initial_delay: Duration) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut message = None;
            let mut show_at = None;
            let mut visible = false;
            let mut frame = 0;
            loop {
                match receiver.recv_timeout(DRAW_INTERVAL) {
                    Ok(Command::Event(Event::Phase(value))) => {
                        message = Some(value);
                        show_at.get_or_insert_with(|| Instant::now() + initial_delay);
                        frame = 0;
                    }
                    Ok(Command::Suspend(done)) => {
                        if visible {
                            clear(&mut output);
                        }
                        message = None;
                        show_at = None;
                        visible = false;
                        let _ = done.send(());
                    }
                    Ok(Command::Event(Event::Suspend)) => unreachable!(),
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if let Some(message) = message
                    .as_ref()
                    .filter(|_| show_at.is_some_and(|deadline| Instant::now() >= deadline))
                {
                    render(&mut output, FRAMES[frame], message);
                    visible = true;
                    frame = (frame + 1) % FRAMES.len();
                }
            }
            if visible {
                clear(&mut output);
            }
        });
        Self {
            sender: Some(sender),
            thread: Some(thread),
        }
    }

    pub fn report(&self, event: Event) {
        if let Some(sender) = &self.sender {
            match event {
                Event::Phase(_) => {
                    let _ = sender.send(Command::Event(event));
                }
                Event::Suspend => {
                    let (done, waiting) = mpsc::channel();
                    if sender.send(Command::Suspend(done)).is_ok() {
                        let _ = waiting.recv();
                    }
                }
            }
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn should_start(enabled: bool, terminal: bool, term: Option<&str>) -> bool {
    enabled && terminal && term != Some("dumb")
}

fn clear(output: &mut impl Write) {
    let mut visible = lock_stderr();
    let _ = write!(output, "\r\x1b[2K");
    let _ = output.flush();
    *visible = false;
}

fn render(output: &mut impl Write, frame: &str, message: &str) {
    let mut visible = lock_stderr();
    let _ = write!(output, "\r\x1b[2K{frame} {message}");
    let _ = output.flush();
    *visible = true;
}

fn lock_stderr() -> MutexGuard<'static, bool> {
    STDERR_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        sync::{Arc, Mutex},
    };

    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn phase_delivers_the_owned_label() {
        let events = RefCell::new(Vec::new());
        phase(
            &|event| events.borrow_mut().push(event),
            "Fetching 2 threads",
        );
        assert_eq!(
            events.into_inner(),
            [Event::Phase("Fetching 2 threads".into())]
        );
    }

    #[test]
    fn disabled_spinner_accepts_progress_without_starting_a_thread() {
        let spinner = Spinner::new(false);
        spinner.report(Event::Phase("Pulling personal".into()));
        spinner.report(Event::Suspend);
        assert!(spinner.thread.is_none());
    }

    #[test]
    fn spinner_starts_only_for_interactive_human_output() {
        assert!(should_start(true, true, None));
        assert!(!should_start(false, true, None));
        assert!(!should_start(true, false, None));
        assert!(!should_start(true, true, Some("dumb")));
    }

    #[test]
    fn active_spinner_renders_and_suspends_before_returning() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let spinner = Spinner::start(SharedBuffer(Arc::clone(&output)), Duration::ZERO);
        spinner.report(Event::Phase("Pulling personal".into()));
        spinner.report(Event::Suspend);
        drop(spinner);

        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(output.contains("⠋ Pulling personal"), "{output:?}");
        assert!(output.ends_with("\r\x1b[2K"), "{output:?}");
    }

    #[test]
    fn fast_phase_finishes_without_terminal_output() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let spinner = Spinner::start(SharedBuffer(Arc::clone(&output)), Duration::from_secs(1));
        spinner.report(Event::Phase("Already finished".into()));
        drop(spinner);

        assert!(output.lock().unwrap().is_empty());
    }
}
