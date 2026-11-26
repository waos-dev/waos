//! What a device answers: its errors and the scopes that catch them, its loss, its polls, its
//! mappings' modes, and a shader's compilation messages.

use wasdk::gpu_types as wit;

use super::enum_conversions;

enum_conversions! {
    error_filter_to_wit, error_filter_from_wit: ErrorFilter <=> ErrorFilter {
        OutOfMemory, Validation, Internal,
    }
}

enum_conversions! {
    device_lost_reason_to_wit, device_lost_reason_from_wit: DeviceLostReason <=> DeviceLostReason {
        Unknown, Destroyed,
    }
}

enum_conversions! {
    poll_status_to_wit, poll_status_from_wit: PollStatus <=> PollStatus {
        QueueEmpty, WaitSucceeded, Poll,
    }
}

enum_conversions! {
    map_mode_to_wit, map_mode_from_wit: MapMode <=> MapMode {
        Read, Write,
    }
}

enum_conversions! {
    compilation_message_type_to_wit, compilation_message_type_from_wit:
        CompilationMessageType <=> CompilationMessageType {
        Error, Warning, Info,
    }
}

/// wgpu-core's error as wgpu's, its source the engine it came from.
pub fn error_from_wit(error: wit::Error) -> wgpu::Error {
    let source = || Box::from("the engine's wgpu-core, through wasdk:gpu");
    match error {
        wit::Error::OutOfMemory => wgpu::Error::OutOfMemory { source: source() },
        wit::Error::Validation(description) => wgpu::Error::Validation {
            source: source(),
            description,
        },
        wit::Error::Internal(description) => wgpu::Error::Internal {
            source: source(),
            description,
        },
    }
}

/// A poll as the WIT asks for one, its timeout in nanoseconds.
pub fn poll_type_to_wit(poll_type: wgpu::wgt::PollType<u64>) -> wit::PollType {
    match poll_type {
        wgpu::wgt::PollType::Poll => wit::PollType::Poll,
        wgpu::wgt::PollType::Wait {
            submission_index,
            timeout,
        } => wit::PollType::Wait((
            submission_index,
            timeout.map(|timeout| u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX)),
        )),
    }
}

pub fn poll_error_from_wit(error: wit::PollError) -> wgpu::PollError {
    match error {
        wit::PollError::Timeout => wgpu::PollError::Timeout,
        wit::PollError::WrongSubmissionIndex((asked, submitted)) => {
            wgpu::PollError::WrongSubmissionIndex(asked, submitted)
        }
    }
}

pub fn compilation_info_from_wit(messages: Vec<wit::CompilationMessage>) -> wgpu::CompilationInfo {
    wgpu::CompilationInfo {
        messages: (messages.into_iter())
            .map(|message| wgpu::CompilationMessage {
                message: message.message,
                message_type: compilation_message_type_from_wit(message.message_type),
                location: message.location.map(|location| wgpu::SourceLocation {
                    line_number: location.line_number,
                    line_position: location.line_position,
                    offset: location.offset,
                    length: location.length,
                }),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_wait_carries_its_submission_and_its_timeout_in_nanoseconds() {
        let wait = wgpu::wgt::PollType::Wait {
            submission_index: Some(7),
            timeout: Some(Duration::from_millis(2)),
        };
        assert!(matches!(
            poll_type_to_wit(wait),
            wit::PollType::Wait((Some(7), Some(2_000_000)))
        ));
    }

    #[test]
    fn a_validation_error_keeps_its_message() {
        let error = error_from_wit(wit::Error::Validation("bad".to_owned()));
        assert!(
            matches!(error, wgpu::Error::Validation { description, .. } if description == "bad")
        );
    }
}
