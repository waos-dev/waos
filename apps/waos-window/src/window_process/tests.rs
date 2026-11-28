use futures::executor::block_on;
use waos_window_messages::encode;

use super::*;
use waos::bindings::wasdk::platform::views::Cursor;
use waos_window_messages::ViewCall;

use crate::resize_waits::WaitEnd;

const WINDOW: u64 = 2;
const SHOWN: [u32; 2] = [800, 600];
const RESIZED: [u32; 2] = [1000, 600];

/// The connection's thread, receiving on a port of the test's own, and the send right to it, which
/// plays WaOS.
struct Receiving {
    waos: SendRight,
    waits: Arc<ResizeWaits>,
    handed: UnboundedReceiver<Event>,
    thread: thread::JoinHandle<()>,
}

fn receiving() -> Receiving {
    let port = ReceiveRight::new().unwrap();
    let waos = port.make_send().unwrap();
    port.notify_when_no_senders().unwrap();
    let waits = Arc::new(ResizeWaits::default());
    let (events, handed) = mpsc::unbounded();
    let recording = Arc::clone(&waits);
    let thread = thread::spawn(move || receive(&port, &events, &recording));
    Receiving {
        waos,
        waits,
        handed,
        thread,
    }
}

impl Receiving {
    fn send(&self, message: &ToWindow) {
        self.waos
            .send(PROTOCOL, &encode(message), Vec::new())
            .unwrap();
    }

    /// WaOS said the view in `WINDOW` shows a picture at `SHOWN`, and the thread recorded it.
    fn showing(self) -> Receiving {
        self.send(&ToWindow::PictureShown {
            window: WINDOW,
            size: SHOWN,
        });
        assert_eq!(self.waits.wait(WINDOW, SHOWN), WaitEnd::Shown);
        self.waits.expect(WINDOW);
        self
    }

    /// Waits for the picture at `RESIZED` on a thread of its own, as the main thread does.
    fn waiting(&self) -> thread::JoinHandle<WaitEnd> {
        let waits = Arc::clone(&self.waits);
        thread::spawn(move || waits.wait(WINDOW, RESIZED))
    }
}

#[test]
fn a_picture_waos_says_is_shown_wakes_the_resize_waiting_for_it_and_goes_no_further() {
    let mut receiving = receiving().showing();
    let waiter = receiving.waiting();
    receiving.send(&ToWindow::PictureShown {
        window: WINDOW,
        size: RESIZED,
    });
    assert_eq!(waiter.join().unwrap(), WaitEnd::Shown);
    receiving.send(&ToWindow::ReadNotificationPermission);
    let handed = block_on(receiving.handed.next());
    assert!(
        matches!(
            handed,
            Some(Event::Received(ToWindow::ReadNotificationPermission, _))
        ),
        "the main thread is handed what came after the pictures, and no picture"
    );
}

#[test]
fn the_programs_end_wakes_a_resize_waiting_for_a_picture() {
    let mut receiving = receiving().showing();
    let waiter = receiving.waiting();
    receiving.send(&ToWindow::ProgramEnded);
    assert_eq!(waiter.join().unwrap(), WaitEnd::Ended);
    let handed = block_on(receiving.handed.next());
    assert!(
        matches!(handed, Some(Event::Received(ToWindow::ProgramEnded, _))),
        "the main thread hears the end too, once it waits no more"
    );
}

#[test]
fn waos_gone_wakes_a_resize_waiting_for_a_picture() {
    let Receiving {
        waos,
        waits,
        mut handed,
        thread,
    } = receiving().showing();
    let waiter = {
        let waits = Arc::clone(&waits);
        thread::spawn(move || waits.wait(WINDOW, RESIZED))
    };
    drop(waos);
    assert_eq!(waiter.join().unwrap(), WaitEnd::Ended);
    assert!(matches!(block_on(handed.next()), Some(Event::Gone(_))));
    thread.join().unwrap();
}

#[test]
fn a_request_the_program_awaits_ends_the_resize_waiting_and_a_one_way_call_does_not() {
    let mut receiving = receiving().showing();
    let waiter = receiving.waiting();
    receiving.send(&ToWindow::View {
        window: WINDOW,
        call: ViewCall::SetCursor(Cursor::Text),
    });
    let handed = block_on(receiving.handed.next());
    assert!(matches!(
        handed,
        Some(Event::Received(ToWindow::View { .. }, _))
    ));
    // The connection's thread records what it hears before it hands it on.
    assert!(
        receiving.waits.awaits(WINDOW),
        "a call on the view ends nothing"
    );
    receiving.send(&ToWindow::OpenFileDialog {
        dialog: 5,
        parent: Some(WINDOW),
        kind: waos_window_messages::FileDialog::OpenFolder(Default::default()),
    });
    assert_eq!(waiter.join().unwrap(), WaitEnd::Requested);
    assert!(
        matches!(
            block_on(receiving.handed.next()),
            Some(Event::Received(ToWindow::OpenFileDialog { .. }, _))
        ),
        "the main thread opens the dialog once it waits no more"
    );
    assert!(
        !receiving.waits.awaits(WINDOW),
        "and nothing waits until it closes"
    );
}
