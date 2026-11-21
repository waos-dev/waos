//! A program that runs parts of its work on workers, other instances of its own code that share
//! nothing with it. It asks for a view, which it draws nothing in, so it can be hidden, then in
//! order: it says how many threads the machine runs at once; starts a worker
//! at an entry it does not have; sends three words to a worker that reverses each, and reads the
//! three back in order; counts primes on a worker while its main instance goes on waking every
//! 10 ms, and says how often it woke before the answer came; starts a worker that traps, and says
//! how it ended; starts a worker that relays each word through a worker of its own; asks a worker
//! what it has of the user's side; drops a counting worker as soon as it counts; and last starts a
//! counting worker and ends, leaving it running, once it is asked to exit.
//!
//! It prints each step as `what: result`; a counting worker prints `worker count <n>` as it
//! counts.
//!
//! ```text
//! cargo build -p workers --target wasm32-wasip3
//! ```

use std::cell::Cell;
use std::rc::Rc;

use wasdk::lifecycle::{self, AppExitResponse, ExitRequest};
use wasdk::workers::{self, Worker, WorkerError, Workers};
use wasdk::{
    StreamReader, StreamWriter, audio, cameras, clipboard, file_dialogs, media_players, menus,
    monotonic_clock, notifications, opener, views, windows, wit_stream,
};

/// The primes counted on a worker are those below this.
const PRIMES_BELOW: u32 = 200_000;

/// How often the main instance wakes while a worker counts primes, in nanoseconds.
const WAKE_EVERY: u64 = 10_000_000;

/// How many steps a counting worker takes between two reports.
const REPORT_EVERY: u64 = 1 << 20;

#[wasdk::main]
async fn main() {
    if let Err(error) = views::request_view(&windows::WindowConfig::new(), None) {
        println!("view: {error}");
    }
    let Some(workers) = workers::get_workers() else {
        println!("workers: none");
        return;
    };
    println!("parallelism: {}", workers.available_parallelism());
    start_an_unknown_entry(&workers).await;
    reverse_words(&workers).await;
    count_primes_while_awake(&workers).await;
    see_a_trap(&workers).await;
    relay_words(&workers).await;
    ask_for_the_users_side(&workers).await;
    drop_a_counting_worker(&workers).await;
    leave_a_worker_counting(&workers).await;
}

async fn start_an_unknown_entry(workers: &Workers) {
    let (_, inbox) = wit_stream::new();
    match workers.start("nothing".to_owned(), inbox).await {
        Err(WorkerError::UnknownEntry(entry)) => println!("unknown entry: {entry}"),
        Err(WorkerError::Refused(why)) => println!("unknown entry: refused, {why}"),
        Ok(_) => println!("unknown entry: started"),
    }
}

async fn reverse_words(workers: &Workers) {
    let (mut words, worker, mut reversed) = start(workers, REVERSE_ENTRY).await;
    for word in ["one", "two", "three"] {
        words.write_one(word.as_bytes().to_vec()).await;
    }
    let mut answers = Vec::new();
    for _ in 0..3 {
        answers.push(text(reversed.next().await));
    }
    drop(words);
    println!("reversed: {}", answers.join(" "));
    println!("reverse finished: {}", ending(&worker).await);
}

async fn count_primes_while_awake(workers: &Workers) {
    let (mut limit, _worker, mut answer) = start(workers, PRIMES_ENTRY).await;
    let wakes = Rc::new(Cell::new(0u32));
    let answered = Rc::new(Cell::new(false));
    wasdk::spawn(wake_until(Rc::clone(&wakes), Rc::clone(&answered)));
    limit.write_one(PRIMES_BELOW.to_le_bytes().to_vec()).await;
    let count = answer.next().await.map(|count| u32_of(&count));
    answered.set(true);
    println!(
        "primes below {PRIMES_BELOW}: {}, the main instance awake {} times meanwhile",
        count.map_or("none".to_owned(), |count| count.to_string()),
        wakes.get()
    );
}

/// Wakes every [`WAKE_EVERY`] and counts it, until `answered`.
async fn wake_until(wakes: Rc<Cell<u32>>, answered: Rc<Cell<bool>>) {
    while !answered.get() {
        monotonic_clock::wait_for(WAKE_EVERY).await;
        wakes.set(wakes.get() + 1);
    }
}

async fn see_a_trap(workers: &Workers) {
    let (_inbox, worker, _messages) = start(workers, TRAP_ENTRY).await;
    println!("trap finished: {}", ending(&worker).await);
}

async fn relay_words(workers: &Workers) {
    let (mut words, worker, mut relayed) = start(workers, RELAY_ENTRY).await;
    words.write_one(b"relay".to_vec()).await;
    let answer = text(relayed.next().await);
    drop(words);
    println!("relayed: {answer}");
    println!("relay finished: {}", ending(&worker).await);
}

async fn ask_for_the_users_side(workers: &Workers) {
    let (_inbox, _worker, mut report) = start(workers, USERS_SIDE_ENTRY).await;
    println!("user's side in a worker: {}", text(report.next().await));
}

async fn drop_a_counting_worker(workers: &Workers) {
    let (_inbox, worker, mut counting) = start(workers, COUNT_ENTRY).await;
    counting.next().await;
    drop(worker);
    println!("dropped: a counting worker");
}

async fn leave_a_worker_counting(workers: &Workers) {
    let (_inbox, _worker, mut counting) = start(workers, COUNT_ENTRY).await;
    let mut exit_requests = lifecycle::exit_requests();
    counting.next().await;
    println!("counting: until asked to exit");
    if let Some(request) = exit_requests.next().await {
        ExitRequest::respond(request, AppExitResponse::Exit);
    }
    println!("exit: with a worker counting");
}

/// A worker started at `entry`: what writes its inbox, the worker, and what it sends.
async fn start(
    workers: &Workers,
    entry: &str,
) -> (StreamWriter<Vec<u8>>, Worker, StreamReader<Vec<u8>>) {
    let (to_worker, inbox) = wit_stream::new();
    let worker = match workers.start(entry.to_owned(), inbox).await {
        Ok(worker) => worker,
        Err(error) => panic!("the worker {entry} did not start: {error:?}"),
    };
    let messages = worker
        .messages()
        .expect("a worker's messages are there once");
    (to_worker, worker, messages)
}

/// How `worker` ended.
async fn ending(worker: &Worker) -> String {
    match worker.finished().await {
        Ok(()) => "ok".to_owned(),
        Err(why) => format!("failed, {}", why.lines().next().unwrap_or_default()),
    }
}

fn text(message: Option<Vec<u8>>) -> String {
    message.map_or("none".to_owned(), |bytes| {
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn u32_of(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("a count is four bytes"))
}

/// Sends back each word it is sent, its letters reversed.
#[wasdk::worker]
async fn reverse(mut words: StreamReader<Vec<u8>>, mut reversed: StreamWriter<Vec<u8>>) {
    while let Some(mut word) = words.next().await {
        word.reverse();
        reversed.write_one(word).await;
    }
}

/// Counts the primes below the number it is sent, and sends the count.
#[wasdk::worker]
async fn primes(mut limit: StreamReader<Vec<u8>>, mut count: StreamWriter<Vec<u8>>) {
    if let Some(limit) = limit.next().await {
        let below = u32_of(&limit);
        let primes = (2..below).filter(|&number| is_prime(number)).count() as u32;
        count.write_one(primes.to_le_bytes().to_vec()).await;
    }
}

fn is_prime(number: u32) -> bool {
    (2..)
        .take_while(|divisor| divisor * divisor <= number)
        .all(|divisor| !number.is_multiple_of(divisor))
}

/// Aborts at once, which in a component is a trap.
#[wasdk::worker]
async fn trap(_inbox: StreamReader<Vec<u8>>, _outbox: StreamWriter<Vec<u8>>) {
    std::process::abort();
}

/// Passes each word through a `reverse` worker of its own, twice, so it comes back as it went.
#[wasdk::worker]
async fn relay(mut words: StreamReader<Vec<u8>>, mut relayed: StreamWriter<Vec<u8>>) {
    let workers = workers::get_workers().expect("a worker has workers");
    let (mut first, first_worker, mut once) = start(&workers, REVERSE_ENTRY).await;
    let (mut second, second_worker, mut twice) = start(&workers, REVERSE_ENTRY).await;
    while let Some(word) = words.next().await {
        first.write_one(word).await;
        let Some(reversed) = once.next().await else {
            break;
        };
        second.write_one(reversed).await;
        let Some(word) = twice.next().await else {
            break;
        };
        relayed.write_one(word).await;
    }
    drop((first, second));
    ending(&first_worker).await;
    ending(&second_worker).await;
}

/// Sends what it finds of the user's side: none of it, each part as a platform without it
/// answers.
#[wasdk::worker]
async fn users_side(_inbox: StreamReader<Vec<u8>>, mut report: StreamWriter<Vec<u8>>) {
    let request = views::request_view(&windows::WindowConfig::new(), None);
    let exit_requests_end = lifecycle::exit_requests().next().await.is_none();
    let found = [
        format!("views {}", views::views().len()),
        format!("request {}", answer_of(request)),
        none_or_some("compositor", views::get_compositor().is_none()),
        none_or_some("clipboard", clipboard::get_clipboard().is_none()),
        none_or_some("menu bar", menus::get_menu_bar().is_none()),
        none_or_some("file dialogs", file_dialogs::get_file_dialogs().is_none()),
        none_or_some(
            "notifications",
            notifications::get_notifications().is_none(),
        ),
        none_or_some("opener", opener::get_opener().is_none()),
        none_or_some("audio", audio::get_audio().is_none()),
        none_or_some(
            "media players",
            media_players::get_media_players().is_none(),
        ),
        none_or_some("cameras", cameras::get_cameras().is_none()),
        format!(
            "exit requests {}",
            if exit_requests_end { "end" } else { "come" }
        ),
    ];
    report.write_one(found.join(", ").into_bytes()).await;
}

fn answer_of(request: Result<views::RequestId, views::ViewError>) -> String {
    match request {
        Ok(_) => "taken".to_owned(),
        Err(views::ViewError::Unsupported) => "unsupported".to_owned(),
        Err(views::ViewError::Failed(why)) => format!("failed, {why}"),
    }
}

fn none_or_some(what: &str, none: bool) -> String {
    format!("{what} {}", if none { "none" } else { "some" })
}

/// Says it counts, then counts for ever, printing its count every [`REPORT_EVERY`] steps.
#[wasdk::worker]
async fn count(_inbox: StreamReader<Vec<u8>>, mut counting: StreamWriter<Vec<u8>>) {
    counting.write_one(b"counting".to_vec()).await;
    let mut count: u64 = 0;
    loop {
        count = std::hint::black_box(count.wrapping_add(1));
        if count.is_multiple_of(REPORT_EVERY) {
            println!("worker count {count}");
        }
    }
}
