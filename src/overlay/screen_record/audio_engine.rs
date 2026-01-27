use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};
use ringbuf::traits::*;
use ringbuf::HeapRb;
use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn record_audio(path: String, stop_signal: Arc<AtomicBool>, finished_signal: Arc<AtomicBool>) {
    thread::spawn(move || {
        let host = match cpal::host_from_id(cpal::HostId::Wasapi) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("Failed to get WASAPI host: {}", e);
                cpal::default_host()
            }
        };

        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                eprintln!("No default output device found for loopback");
                finished_signal.store(true, Ordering::SeqCst);
                return;
            }
        };

        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to get default output config: {}", e);
                finished_signal.store(true, Ordering::SeqCst);
                return;
            }
        };

        // Create a ring buffer for audio data
        let buffer_len = 4 * 1024 * 1024; // ~4 million samples
        let rb = HeapRb::<f32>::new(buffer_len);
        let (mut producer, mut consumer) = rb.split();

        let stream_config: cpal::StreamConfig = config.clone().into();
        let channels = stream_config.channels;
        let sample_rate = stream_config.sample_rate;

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        // Capture in float, but we will convert to 16-bit integer
        let stream = match device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                let _ = producer.push_slice(data);
            },
            err_fn,
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to build audio input stream: {}", e);
                finished_signal.store(true, Ordering::SeqCst);
                return;
            }
        };

        if let Err(e) = stream.play() {
            eprintln!("Failed to start audio stream: {}", e);
            finished_signal.store(true, Ordering::SeqCst);
            return;
        }

        println!(
            "Audio recording started (16-bit PCM): {} (Rate: {}, Channels: {})",
            path, sample_rate, channels
        );

        // Use 16-bit Signed Integer (PCM) instead of Float.
        // This is much more compatible and less prone to "static pops"
        let spec = WavSpec {
            channels: channels as u16,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let file = match File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create audio file: {}", e);
                return;
            }
        };

        let buf_writer = BufWriter::new(file);
        let mut writer = match WavWriter::new(buf_writer, spec) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Failed to create WAV writer: {}", e);
                return;
            }
        };

        let mut chunk = vec![0.0f32; 16384];

        while !stop_signal.load(Ordering::SeqCst) {
            if consumer.is_empty() {
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            let count = consumer.pop_slice(&mut chunk);
            if count > 0 {
                for i in 0..count {
                    // Hard clamp to [-1.0, 1.0] and convert to i16
                    // This eliminates floating point range issues causing pops
                    let sample = chunk[i].clamp(-1.0, 1.0);
                    let pcm_sample = (sample * 32767.0) as i16;

                    if let Err(e) = writer.write_sample(pcm_sample) {
                        eprintln!("WAV Write error: {}", e);
                        break;
                    }
                }
            }
        }

        println!("Audio stop signal received. Flushing buffer...");
        drop(stream);

        // Flush remainder
        loop {
            let count = consumer.pop_slice(&mut chunk);
            if count == 0 {
                break;
            }
            for i in 0..count {
                let sample = chunk[i].clamp(-1.0, 1.0);
                let pcm_sample = (sample * 32767.0) as i16;
                let _ = writer.write_sample(pcm_sample);
            }
        }

        if let Err(e) = writer.finalize() {
            eprintln!("Failed to finalize WAV file: {}", e);
        } else {
            println!("Audio recording finished: {}", path);
        }

        finished_signal.store(true, Ordering::SeqCst);
    });
}
