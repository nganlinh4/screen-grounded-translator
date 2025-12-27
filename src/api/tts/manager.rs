use super::types::{AudioEvent, QueuedRequest, TtsRequest};
use super::utils;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Condvar, Mutex,
};

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Manages the persistent TTS WebSocket connection
pub struct TtsManager {
    /// Flag to indicate if the connection is ready
    _is_ready: AtomicBool,

    /// Queue for Socket Workers: (Request + Generation, Output Channel)
    pub work_queue: Mutex<VecDeque<(QueuedRequest, mpsc::Sender<AudioEvent>)>>,
    /// Signal for Socket Workers
    pub work_signal: Condvar,

    /// Queue for Player: (Input Channel, Window Handle, Request ID, Generation ID, IsRealtime)
    pub playback_queue: Mutex<VecDeque<(mpsc::Receiver<AudioEvent>, isize, u64, u64, bool)>>,
    /// Signal for Player
    pub playback_signal: Condvar,

    /// Generation counter for interrupts (incrementing this invalidates old jobs)
    pub interrupt_generation: AtomicU64,

    /// Flag to shutdown the manager
    pub shutdown: AtomicBool,
}

impl TtsManager {
    pub fn new() -> Self {
        Self {
            _is_ready: AtomicBool::new(false),
            work_queue: Mutex::new(VecDeque::new()),
            work_signal: Condvar::new(),
            playback_queue: Mutex::new(VecDeque::new()),
            playback_signal: Condvar::new(),
            interrupt_generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        }
    }

    /// Check if TTS is ready to accept requests
    pub fn _is_ready(&self) -> bool {
        self._is_ready.load(Ordering::SeqCst)
    }

    /// Request TTS for the given text. Appends to queue (sequential playback).
    /// Returns the request ID.
    pub fn speak(&self, text: &str, hwnd: isize) -> u64 {
        self.speak_internal(text, hwnd, false)
    }

    /// Request TTS for realtime translation. Uses REALTIME_TTS_SPEED and auto-catchup.
    /// Returns the request ID.
    pub fn speak_realtime(&self, text: &str, hwnd: isize) -> u64 {
        self.speak_internal(text, hwnd, true)
    }

    /// Internal speak implementation
    fn speak_internal(&self, text: &str, hwnd: isize, is_realtime: bool) -> u64 {
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let current_gen = self.interrupt_generation.load(Ordering::SeqCst);

        let (tx, rx) = mpsc::channel();

        // Add to queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.push_back((
                QueuedRequest {
                    req: TtsRequest {
                        _id: id,
                        text: text.to_string(),
                        hwnd,
                        is_realtime,
                    },
                    generation: current_gen,
                },
                tx,
            ));
        }
        self.work_signal.notify_one();

        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id, current_gen, is_realtime));
        }
        self.playback_signal.notify_one();

        id
    }

    /// Request TTS for the given text, interrupting any current speech.
    /// Clears the queue and stops current playback immediately.
    pub fn speak_interrupt(&self, text: &str, hwnd: isize) -> u64 {
        // Increment generation to invalidate all currently running/queued work
        let new_gen = self.interrupt_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

        // Clear all queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.clear();
        }
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.clear(); // Drops receivers, causing senders to error and workers to reset
        }

        // Push new request
        let (tx, rx) = mpsc::channel();

        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.push_back((
                QueuedRequest {
                    req: TtsRequest {
                        _id: id,
                        text: text.to_string(),
                        hwnd,
                        is_realtime: false,
                    },
                    generation: new_gen,
                },
                tx,
            ));
        }
        self.work_signal.notify_one();

        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id, new_gen, false));
        }
        // Force notify player to wake up and check generation/queue
        self.playback_signal.notify_one();

        id
    }

    /// Stop the current speech or cancel pending request
    pub fn stop(&self) {
        self.interrupt_generation.fetch_add(1, Ordering::SeqCst);

        // Clear queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.clear();
        }
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.clear();
        }

        // Wake up player to realize it should stop
        self.playback_signal.notify_all();
    }

    /// Stop speech for a specific request ID (only if it's the current one)
    pub fn stop_if_active(&self, _request_id: u64) {
        // Simplified to just stop
        self.stop();
    }

    /// Check if this request ID is currently active
    pub fn is_speaking(&self, _request_id: u64) -> bool {
        false
    }

    /// Shutdown the TTS manager
    pub fn _shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.interrupt_generation.fetch_add(1, Ordering::SeqCst);
        self.work_signal.notify_all();
        self.playback_signal.notify_all();
    }

    /// List available audio output devices (ID, Name)
    pub fn get_output_devices() -> Vec<(String, String)> {
        utils::get_output_devices()
    }
}
