use clap::Parser;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::{
    env::current_dir,
    path::PathBuf,
    process::Child,
    sync::{
        atomic::AtomicBool,
        mpsc::{channel, Sender},
        Arc,
    },
    time::Duration,
};

#[derive(Parser)]
#[command(name = "WatchAndRerun")]
#[command(version = "1.0")]
#[command(about = "Watch files changes and re-run", long_about = None)]
struct Cli {
    binary_path: PathBuf,
    #[arg(long, short)]
    watch_dir: Option<PathBuf>,
    #[arg(long, short, default_value = "250")]
    debounce_ms: u64,
}

fn main() {
    let Cli {
        binary_path,
        watch_dir,
        debounce_ms,
    } = Cli::parse();

    let debounce_ms = Duration::from_millis(debounce_ms);
    let cwd = current_dir().unwrap();
    let binary_path = cwd.join(binary_path);
    let watch_dir = match watch_dir {
        Some(watch_dir) => cwd.join(watch_dir),
        None => binary_path.parent().unwrap().to_path_buf(),
    };

    let (sender, receiver) = channel::<Event>();
    let mut debouncer = new_debouncer(debounce_ms, {
        let sender = sender.clone();
        move |_res: DebounceEventResult| {
            sender.send(Event::FileChanged).unwrap();
        }
    })
    .unwrap();

    debouncer
        .watcher()
        .watch(&watch_dir, RecursiveMode::Recursive)
        .unwrap();

    let mut child_handle: Option<Child> = None;
    let mut timeout_handle: Option<TimeoutHandle> = None;
    sender.send(Event::Timeout).unwrap();
    loop {
        match receiver.recv().unwrap() {
            Event::FileChanged => {
                if let Some(timeout_handle) = timeout_handle.as_mut() {
                    timeout_handle.abort();
                }
                timeout_handle = Some(timeout(sender.clone(), debounce_ms));
            }
            Event::Timeout => {
                if let Some(child_handle) = child_handle.as_mut() {
                    child_handle.kill().unwrap();
                }
                child_handle = Some(std::process::Command::new(&binary_path).spawn().unwrap());
                println!("Change detected, re-running...");
            }
        }
    }
}

enum Event {
    FileChanged,
    Timeout,
}

fn timeout(sender: Sender<Event>, duration: Duration) -> TimeoutHandle {
    let aborted = Arc::new(AtomicBool::new(false));
    let _ = std::thread::spawn({
        let aborted = aborted.clone();
        move || {
            std::thread::sleep(duration);
            if aborted.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            sender.send(Event::Timeout).unwrap();
        }
    });
    TimeoutHandle { aborted }
}

struct TimeoutHandle {
    aborted: Arc<AtomicBool>,
}
impl TimeoutHandle {
    fn abort(&self) {
        self.aborted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
