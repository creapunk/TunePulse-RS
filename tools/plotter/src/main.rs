use crossbeam_queue::ArrayQueue;
use eframe::{run_native, App, NativeOptions};
use egui::Color32;
use egui_plot::{Plot, Points};
use probe_rs::rtt::Rtt;
use probe_rs::{Permissions, Probe};
use std::time::Duration;
use std::time::Instant;
use std::{
    sync::{Arc, Mutex},
    thread,
};

const HISTORY_LENGTH: usize = 10000;
const STRUCT_SIZE: usize = core::mem::size_of::<RawDataPoint>();
const BUFFER_MULTIPLE: usize = 32;
const BUFFER_SIZE: usize = STRUCT_SIZE * BUFFER_MULTIPLE;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RawDataPoint {
    id: u8,
    timestamp: u32,
    value: f32,
}

struct ProcessedDataPoint {
    id: u8,
    time: f32,
    data: f32,
}

struct PlotApp {
    data_queue: Arc<ArrayQueue<RawDataPoint>>,
    paused: Arc<Mutex<bool>>,
    display_data: Vec<ProcessedDataPoint>,
    visible_ids: std::collections::HashSet<u8>,
    known_ids: std::collections::HashSet<u8>,
    history_length: usize,
}

impl ProcessedDataPoint {
    fn new(time: f32, id: u8, data: f32) -> Self {
        Self { time, id, data }
    }

    fn to_point(&self) -> Points {
        Points::new(vec![[self.time as f64, self.data as f64]])
    }

    fn to_point_with_color(&self, color: Color32) -> Points {
        Points::new(vec![[self.time as f64, self.data as f64]]).color(color)
    }

    fn from_raw(raw: &RawDataPoint) -> Self {
        Self {
            time: raw.timestamp as f32,
            id: raw.id,
            data: raw.value,
        }
    }
}

impl App for PlotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Add controls panel above the plot
            ui.horizontal(|ui| {
                if ui
                    .button(if *self.paused.lock().unwrap() {
                        "Resume"
                    } else {
                        "Pause"
                    })
                    .clicked()
                {
                    let mut paused = self.paused.lock().unwrap();
                    *paused = !*paused;
                }

                // Add history length slider
                ui.add(
                    egui::Slider::new(&mut self.history_length, 100..=50000)
                        .text("History Length")
                        .logarithmic(true),
                );

                // Collect unique IDs from display data
                let unique_ids: std::collections::HashSet<u8> =
                    self.display_data.iter().map(|point| point.id).collect();

                for id in unique_ids {
                    self.known_ids.insert(id);
                }

                // Add toggle buttons for each ID
                // Use known_ids instead of scanning display data
                for &id in self.known_ids.iter() {
                    let mut visible = self.visible_ids.contains(&id);
                    if ui.checkbox(&mut visible, format!("ID {}", id)).changed() {
                        if visible {
                            self.visible_ids.insert(id);
                        } else {
                            self.visible_ids.remove(&id);
                        }
                    }
                }
            });

            // Drain queue into display buffer when not paused
            if !*self.paused.lock().unwrap() {
                while let Some(point) = self.data_queue.pop() {
                    self.display_data.push(ProcessedDataPoint::from_raw(&point));
                }

                // Maintain history length
                if self.display_data.len() > self.history_length {
                    self.display_data
                        .drain(0..self.display_data.len() - self.history_length);
                }
            }

            Plot::new("Real-time Data")
                .view_aspect(2.0)
                .show(ui, |plot_ui| {
                    // Only show points for visible IDs
                    for point in &self.display_data {
                        if self.visible_ids.contains(&point.id) {
                            plot_ui.points(point.to_point_with_color(id_to_color(point.id)));
                        }
                    }
                });
        });

        if !*self.paused.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

fn connect_and_read(
    data_queue: Arc<ArrayQueue<RawDataPoint>>,
    paused: Arc<Mutex<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let probe = Probe::list_all()[0].open()?;
    let mut session = probe.attach("STM32G431CBTx", Permissions::default())?;
    let memory_map = session.target().memory_map.clone();
    let mut core = session.core(0)?;
    let mut rtt = Rtt::attach(&mut core, &memory_map)?;

    let mut buf = vec![0u8; BUFFER_SIZE]; // Increased buffer size

    // Get the channel once, outside the loop
    let channel = rtt
        .up_channels()
        .take(0)
        .ok_or("Failed to get RTT channel")?;

    loop {
        let loop_start = Instant::now();
        let mut upload_start = Instant::now();

        if paused.try_lock().map(|guard| *guard).unwrap_or(false) {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        let read_start = Instant::now();
        match channel.read(&mut core, &mut buf) {
            Ok(count) => {
                let num_points = count / STRUCT_SIZE;

                let conversion_start = Instant::now();

                if num_points > 0 {
                    // Process data in chunks for better efficiency
                    let points = unsafe {
                        std::slice::from_raw_parts(
                            buf[..count].as_ptr() as *const RawDataPoint,
                            num_points,
                        )
                    };

                    upload_start = Instant::now();

                    for point in points {
                        if data_queue.push(*point).is_err() {
                            // Queue is full, might want to log this
                            break;
                        }
                    }
                }

                let finish = Instant::now();

                // println!(
                //     "Loop: {:?}, Read: {:?}, Convert: {:?}, Upload: {:?}, Finished: {:?}",
                //     loop_start.elapsed(),
                //     read_start.elapsed(),
                //     conversion_start.elapsed(),
                //     upload_start.elapsed(),
                //     finish.elapsed()
                // );
            }
            Err(e) => {
                eprintln!("Error reading RTT channel: {:?}", e);
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn id_to_color(id: u8) -> Color32 {
    // Use the golden ratio to get a good distribution of hues
    let hue = (id as f32 / 2.0) % 1.0;
    // Keep saturation and value high for good visibility
    let (r, g, b) = hsv_to_rgb(hue, 0.8, 0.9);
    Color32::from_rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };

    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn main() {
    let data_queue = Arc::new(ArrayQueue::new(BUFFER_SIZE));
    let paused = Arc::new(Mutex::new(false));

    let data_queue_clone = data_queue.clone();
    let paused_clone = paused.clone();

    thread::spawn(move || {
        if let Err(e) = connect_and_read(data_queue_clone, paused_clone) {
            eprintln!("Error in data collection: {:?}", e);
        }
    });

    let app = PlotApp {
        data_queue,
        paused,
        display_data: Vec::with_capacity(HISTORY_LENGTH),
        visible_ids: std::collections::HashSet::new(),
        known_ids: std::collections::HashSet::new(),
        history_length: HISTORY_LENGTH,
    };

    let options = NativeOptions::default();
    run_native("Real-time Plot", options, Box::new(|_cc| Ok(Box::new(app)))).unwrap();
}
