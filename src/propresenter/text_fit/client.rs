//! Process ownership and request/response correlation for `TextKit` measurement.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use super::evidence::{validate_evidence, TextFitContractSummary, TextFitEvidence};
use super::request::TextFitRequest;
use super::wire::{map_helper_error, WireRequest, WireResponse};
use super::TEXT_FIT_PROTOCOL_VERSION;

#[cfg(target_os = "macos")]
mod bundled_helper;
mod error;

pub use error::TextFitError;

const MAX_RESPONSE_FRAME_BYTES: usize = 1024 * 1024;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

enum HelperFrame {
    Line(Vec<u8>),
    EndOfStream,
    ReadError(std::io::Error),
    TooLarge,
    Truncated,
}

fn read_helper_frames(stdout: ChildStdout, sender: &SyncSender<HelperFrame>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut frame = Vec::new();
        let read = reader
            .by_ref()
            .take(u64::try_from(MAX_RESPONSE_FRAME_BYTES + 1).unwrap_or(u64::MAX))
            .read_until(b'\n', &mut frame);
        let event = match read {
            Ok(0) => HelperFrame::EndOfStream,
            Ok(_) if frame.len() > MAX_RESPONSE_FRAME_BYTES => HelperFrame::TooLarge,
            Ok(_) if frame.last() != Some(&b'\n') => HelperFrame::Truncated,
            Ok(_) => HelperFrame::Line(frame),
            Err(source) => HelperFrame::ReadError(source),
        };
        let terminal = !matches!(event, HelperFrame::Line(_));
        if sender.send(event).is_err() || terminal {
            return;
        }
    }
}

/// A persistent native `TextKit` helper session.
pub struct NativeTextFitOracle {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    responses: Option<Receiver<HelperFrame>>,
    reader_thread: Option<JoinHandle<()>>,
    response_timeout: Duration,
    poisoned: bool,
    next_request_id: u64,
    cache: BTreeMap<Vec<u8>, TextFitEvidence>,
    contract: TextFitContractSummary,
}

impl NativeTextFitOracle {
    /// Start the helper compiled and embedded into this `ProFlow` build.
    pub(crate) fn start_bundled() -> Result<Self, TextFitError> {
        #[cfg(not(target_os = "macos"))]
        {
            return Err(TextFitError::UnsupportedPlatform(
                std::env::consts::OS.to_string(),
            ));
        }
        #[cfg(target_os = "macos")]
        {
            Self::start(&bundled_helper::materialize()?)
        }
    }

    /// Start a checked helper session from an explicit executable path.
    pub(crate) fn start(executable: &Path) -> Result<Self, TextFitError> {
        #[cfg(not(target_os = "macos"))]
        {
            return Err(TextFitError::UnsupportedPlatform(
                std::env::consts::OS.to_string(),
            ));
        }
        #[cfg(target_os = "macos")]
        {
            Self::start_with_contract(
                executable,
                bundled_helper::contract(executable)?,
                RESPONSE_TIMEOUT,
            )
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(super) fn start_with_timeout(
        executable: &Path,
        response_timeout: Duration,
    ) -> Result<Self, TextFitError> {
        Self::start_with_contract(
            executable,
            bundled_helper::contract(executable)?,
            response_timeout,
        )
    }

    #[cfg(target_os = "macos")]
    fn start_with_contract(
        executable: &Path,
        contract: TextFitContractSummary,
        response_timeout: Duration,
    ) -> Result<Self, TextFitError> {
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| TextFitError::HelperUnavailable {
                executable: executable.to_path_buf(),
                source,
            })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            TextFitError::HelperProtocol("helper did not expose standard input".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            TextFitError::HelperProtocol("helper did not expose standard output".to_string())
        })?;
        let (sender, responses) = mpsc::sync_channel(1);
        let reader_thread = match std::thread::Builder::new()
            .name("proflow-text-fit-reader".to_string())
            .spawn(move || read_helper_frames(stdout, &sender))
        {
            Ok(reader_thread) => reader_thread,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TextFitError::StartResponseReader(source));
            }
        };
        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            responses: Some(responses),
            reader_thread: Some(reader_thread),
            response_timeout,
            poisoned: false,
            next_request_id: 1,
            cache: BTreeMap::new(),
            contract,
        })
    }

    /// Return the immutable identity of this native measurement session.
    pub(crate) const fn contract(&self) -> &TextFitContractSummary {
        &self.contract
    }

    /// Measure one request and validate all returned evidence.
    pub(crate) fn measure(
        &mut self,
        request: &TextFitRequest,
    ) -> Result<TextFitEvidence, TextFitError> {
        if self.poisoned {
            return Err(TextFitError::SessionPoisoned);
        }
        let cache_key = serde_json::to_vec(&WireRequest::from_request(0, request))
            .map_err(TextFitError::EncodeRequest)?;
        if let Some(evidence) = self.cache.get(&cache_key) {
            return Ok(evidence.clone());
        }
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(TextFitError::RequestSequenceExhausted)?;
        let wire = WireRequest::from_request(request_id, request);
        serde_json::to_writer(&mut self.stdin, &wire).map_err(TextFitError::EncodeRequest)?;
        if let Err(source) = self
            .stdin
            .write_all(b"\n")
            .and_then(|()| self.stdin.flush())
        {
            self.poison();
            return Err(TextFitError::WriteRequest(source));
        }

        let received = self
            .responses
            .as_ref()
            .ok_or(TextFitError::SessionPoisoned)?
            .recv_timeout(self.response_timeout);
        let frame = match received {
            Ok(HelperFrame::Line(frame)) => frame,
            Ok(HelperFrame::ReadError(source)) => {
                self.poison();
                return Err(TextFitError::ReadResponse(source));
            }
            Ok(HelperFrame::TooLarge) => {
                self.poison();
                return Err(TextFitError::ResponseFrameTooLarge {
                    limit: MAX_RESPONSE_FRAME_BYTES,
                });
            }
            Ok(HelperFrame::Truncated) => {
                self.poison();
                return Err(TextFitError::TruncatedResponseFrame);
            }
            Ok(HelperFrame::EndOfStream) | Err(RecvTimeoutError::Disconnected) => {
                let status = self.child.try_wait().map_err(TextFitError::ReadResponse)?;
                self.poison();
                return Err(TextFitError::HelperTerminated { status });
            }
            Err(RecvTimeoutError::Timeout) => {
                let timeout = self.response_timeout;
                self.poison();
                return Err(TextFitError::ResponseTimeout { timeout });
            }
        };
        let evidence = match Self::decode_response(request_id, request, &frame) {
            Ok(evidence) => evidence,
            Err(error) => {
                self.poison();
                return Err(error);
            }
        };
        self.cache.insert(cache_key, evidence.clone());
        Ok(evidence)
    }

    pub(super) fn decode_response(
        request_id: u64,
        request: &TextFitRequest,
        line: impl AsRef<[u8]>,
    ) -> Result<TextFitEvidence, TextFitError> {
        let response: WireResponse =
            serde_json::from_slice(line.as_ref()).map_err(TextFitError::DecodeResponse)?;
        if response.protocol_version != TEXT_FIT_PROTOCOL_VERSION {
            return Err(TextFitError::ProtocolVersionMismatch {
                expected: TEXT_FIT_PROTOCOL_VERSION,
                actual: response.protocol_version,
            });
        }
        if response.request_id != request_id {
            return Err(TextFitError::ResponseIdMismatch {
                expected: request_id,
                actual: response.request_id,
            });
        }
        match (response.status.as_str(), response.evidence, response.error) {
            ("ok", Some(evidence), None) => validate_evidence(evidence, request),
            ("error", None, Some(error)) => Err(map_helper_error(error)),
            _ => Err(TextFitError::HelperProtocol(
                "response must contain exactly one matching status payload".to_string(),
            )),
        }
    }

    fn poison(&mut self) {
        self.poisoned = true;
        self.responses = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader_thread.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for NativeTextFitOracle {
    fn drop(&mut self) {
        self.poison();
    }
}
