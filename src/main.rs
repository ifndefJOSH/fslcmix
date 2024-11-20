#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use eframe::egui::*;

const PEAK_HOLD_TIME: usize = 4000;
const DECAY_FACTOR: f32 = 0.9999;

use std::{process::exit, sync::{Arc, Mutex}};

fn main() -> eframe::Result {
	let args = Args::parse();
	let shared_mix = Arc::new(Mutex::new(FslcMix::new(args.channels)));
	let app = MixApp {
		mix : shared_mix.clone(),
	};
	if let Ok((client, _status)) = jack::Client::new("fslcmix", jack::ClientOptions::default()) {
		let options = eframe::NativeOptions {
			viewport: egui::ViewportBuilder::default()
				.with_inner_size([500.0, 350.0])
				.with_min_inner_size([300.0, 350.0])
				.with_max_inner_size([5000.0, 350.0]),
			..Default::default()
		};

		let process_callback = register_jack_callback(&client, shared_mix);
		// Create process and activate the client
		let process = jack::contrib::ClosureProcessHandler::new(process_callback);
		let active_client = client.activate_async((), process).unwrap();
		let result = eframe::run_native(
			"FSLCMix", 
			options,
			Box::new(|cc| {
				// Dark theme
				cc.egui_ctx.set_theme(egui::Theme::Dark);
				// egui_extras::install_image_loaders(&cc.egui_ctx);
				Ok(Box::new(app))
			}),
		);
		if let Err(err) = active_client.deactivate() {
			eprintln!("JACK exited with error: {err}");
		}
		result
	}
	else {
		eprintln!("Could not connect to JACK Audio Server.");
		let options = eframe::NativeOptions {
			viewport: egui::ViewportBuilder::default()
				.with_inner_size([150.0, 80.0])
				.with_min_inner_size([150.0, 80.0])
				.with_max_inner_size([150.0, 80.0]),
			..Default::default()
		};

		let result = eframe::run_native(
			"Error - FSLCMix",
			options,
			Box::new(|cc| {
				cc.egui_ctx.set_theme(egui::Theme::Dark);
				Ok(Box::new(ErrorBox { text: "Could not connect to JACK Audio Server".to_owned(), }))
			}),
		);
		result
	}
}

fn register_jack_callback(client: &jack::Client, mixer: Arc<Mutex<FslcMix>>) -> impl FnMut(&jack::Client, &jack::ProcessScope) -> jack::Control  {
	let unlocked_mixer = mixer.lock().unwrap();
	let in_ports = unlocked_mixer.channels.iter().map(
		|channel| channel.declare_jack_port(&client)).collect::<Vec<_>>();
	let mut out_port = client.register_port("Master Out", jack::AudioOut::default()).unwrap();
	let process_callback = {
		let mixer = Arc::clone(&mixer);
		move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
			let ins = in_ports.iter().map(|port| port.as_slice(ps)).collect::<Vec<_>>();
			let out = out_port.as_mut_slice(ps);
			if let Ok(mut owned_mixer) = mixer.lock() {
				owned_mixer.mix(ins, out);
			} else {
				eprintln!("Could not gain access to mutex!");
			}
			jack::Control::Continue
		}
	};
	process_callback
}

fn db_peak(val : f32) -> f32 {
	20.0 * val.log10()
}

fn db_rms(val : f32) -> f32 {
	10.0 * val.log10()
}

struct ErrorBox {
	text: String,
}

impl eframe::App for ErrorBox {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
	    egui::CentralPanel::default().show(ctx, |ui| {
			ui.vertical(|ui| {
				ui.label(&self.text);
				ui.horizontal(|ui| {
					if ui.button("Okay").clicked() {
						exit(1);
					}
				});
			});
		});
	}
}

struct MixApp {
	mix: Arc<Mutex<FslcMix>>,
}

impl eframe::App for MixApp {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
		let mut owned_mix = self.mix.lock().unwrap();
		owned_mix.update(ctx, frame);
	}
}

/// A mixer that supports an arbitrary number of channels.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
	/// Number of channels
	#[arg(short, long, default_value_t = 5)]
	channels: u8,
}

struct FslcMix {
	channels: Vec<MixChannel>,
	master: MixChannel,
	normalize: bool,
	ui_size: egui::Vec2,
	max_gain: f32,
}

impl FslcMix {
	fn new(num_channels: u8) -> Self {
		Self { 
			channels: (1..=num_channels).map(|i| 
				MixChannel { 
					channel_name: format!("Channel {}", i), 
					..Default::default() 
				} 
			).collect(),
			master: MixChannel { 
				channel_name: "Master".to_owned(), 
				..Default::default() 
			},
			normalize: false,
			ui_size: egui::Vec2::new(400.0, 330.0), // This size doesn't matter since it's
													// overritten
			max_gain: 1.25,
		}
	}

	fn mix(&mut self, inputs : Vec<&[f32]>, output : &mut [f32]) {
		// Sanity check
		assert!(inputs.len() == self.channels.len());
		// Initialize to zeros
		for i in 0..output.len() {
			output[i] = 0.0;
		}
		if self.master.mute {
			return;
		}
		// TODO: short circuit
		let any_solo = self.channels.iter()
			.fold(false, |any_so_far, channel| {
				channel.solo || any_so_far
			});
		// Mix each channel together
		for channel_index in 0..inputs.len() {
			let channel = &mut self.channels[channel_index];
			let input = inputs[channel_index];
			channel.mix(input, output, any_solo);
		}
		// Apply the master channel's mix and normalize if necessary
		let norm_factor = inputs.len() as f32;
		// Bypass its mix() function since we do it slightly different here
		self.master.rms(output);
		for i in 0..output.len() {
			if self.normalize {
				output[i] /= norm_factor;
			}
			let sample = output[i] * self.master.gain;
			let out_sample = if self.master.limit && sample >= 1.0 {
				1.0
			} else if self.master.limit && sample <= -1.0 {
				-1.0
			} else {
				sample
			};
			if out_sample > self.master.max {
				self.master.max = out_sample;
			}
			output[i] = out_sample;

			self.master.update_smoothed(out_sample);
		}
		self.master.last = output[output.len() - 1];
	}

	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
	   //egui::Window::new("Mixer (FSLCMix)")
		//	.default_pos([100.0, 100.0])
		//	.title_bar(true)
		egui::CentralPanel::default().show(ctx, |ui|{
			ui.vertical(|ui| {
				ui.horizontal(|ui| {
					// ui.label("Licensed under the GPLv3.");
					ui.label("Max Gain:");
					let slider = ui.add(egui::DragValue::new(&mut self.max_gain).range(1.1..=2.0));
					if slider.changed() {
						self.update_max_gain();
					}
					let btn = ui.button("Reset");
					if btn.clicked() {
						self.max_gain = 1.2;
						self.update_max_gain();
					}
					ui.toggle_value(&mut self.normalize, "Normalize");
					if ui.add(egui::Button::new("All Unmute").frame(false)).clicked() {
						for channel in &mut self.channels {
							channel.mute = false;
						}
					}
					if ui.add(egui::Button::new("All Unsolo").frame(false)).clicked() {
						for channel in &mut self.channels {
							channel.solo = false;
						}
					}
				});
			});
			ui.horizontal(|ui| {
				self.master.ui(ui);
				ui.separator();
				egui::ScrollArea::horizontal().show(ui, |ui| {
					for channel in &mut self.channels {
						channel.ui(ui);
					}
				});
			});
			self.ui_size = ctx.used_size();
			// let window_size = self.ui_size + egui::vec2(20.0, 40.0);
		});
		ctx.request_repaint();
		// frame.set_window_size(window_size);
		// frame.request_repaint();
	}

	fn update_max_gain(&mut self) {
		// update max gain for all channels
		for channel in &mut self.channels {
			channel.max_gain = self.max_gain;
		}
		self.master.max_gain = self.max_gain;

	}
}

struct MixChannel {
	gain: f32,
	last: f32,
	last_smoothed: f32,
	peak_hold_counter: usize,
	max: f32,
	last_rms: f32,
	channel_name: String,
	limit: bool,
	mute: bool,
	solo: bool,
	others_solo: bool,
	show_rms: bool,
	max_gain: f32,
}

impl MixChannel {

	fn mix(&mut self, input : &[f32], output : &mut [f32], any_solo : bool) {
		// if self.mute || (any_solo && !self.solo) {
		// 	self.last = 0.0;
		// 	return;
		// }
		self.others_solo = any_solo;
		// Sanity check
		assert!(input.len() == output.len());
		self.rms(input);
		for i in 0..input.len() {
			let sample = input[i] * self.gain;
			let out_sample = if self.limit && sample >= 1.0 {
				1.0
			} else if self.limit && sample <= -1.0 {
				-1.0
			} else {
				sample
			};
			if out_sample > self.max {
				self.max = out_sample;
			}
			// Only mix into the output if we're not muted or no other tracks have solo
			if !(self.mute || (any_solo && !self.solo)) {
				output[i] += out_sample;
			}
			self.last = out_sample;
			self.update_smoothed(self.last);
		}
	}

	fn ui(&mut self, ui : &mut egui::Ui) {
		ui.vertical(|ui| {
			ui.vertical(|ui| {
				let wrap_mode = TextWrapMode::Extend;
				let pb = ui.add(egui::Button::new(
					format!("Peak: {:+2.2} dB", db_peak(self.max)))
					.frame(false)
					.small()
					.wrap_mode(wrap_mode));
				if pb.clicked() {
					self.max = 0.0;
				}
				let rb = ui.add(egui::Button::new(
					format!("RMS: {:+2.2} dB", db_rms(self.last_rms)))
					.frame(false)
					.small()
					.wrap_mode(wrap_mode));
				if rb.clicked() {
					self.last_rms = 0.0;
				}
			});
			ui.horizontal(|ui| {
				ui.spacing_mut().slider_width = 175.0;
				ui.add(egui::Slider::new(&mut self.gain, 0.0..=self.max_gain)
					//.text("Gain")
					.vertical()
					.max_decimals(2));
				// ui.add(egui::ProgressBar::new(self.last));
				self.levels_bar(ui);
			});
			
			ui.horizontal(|ui| {
				let btn = ui.button("Reset");
				if btn.clicked() {
					self.gain = 1.0;
				}
				ui.toggle_value(&mut self.show_rms, "RMS");
			});
			ui.horizontal(|ui| {
				ui.toggle_value(&mut self.mute, "M");
				ui.toggle_value(&mut self.solo, "S");
				ui.toggle_value(&mut self.limit, "Lim");
			});
			ui.add(egui::TextEdit::singleline(&mut self.channel_name).desired_width(85.0));
		});
	}

	fn levels_bar(&self, ui: &mut Ui) {
		// TODO: log scale so dB looks nice
		let val = if self.show_rms { 
			self.last_rms 
		} else {
			self.last_smoothed
		};
		let val_db = if self.show_rms {
			db_rms(val)
		} else {
			db_peak(val)
		};
		let (rect, response) = ui.allocate_exact_size(vec2(10.0, 190.0), egui::Sense::hover()); 
		let painter = ui.painter(); 
		let filled_height = (rect.height() * val / self.max_gain).min(rect.height()); // Show a bit over max amplitude 
		// let filled_rect = Rect::from_min_max(rect.min, rect.min + vec2(rect.width(), filled_height)); 
		// let remaining_rect = Rect::from_min_max(filled_rect.max, rect.max);
		let filled_rect = Rect::from_min_max(rect.max - vec2(rect.width(), filled_height), rect.max);
		// let remaining_rect = Rect::from_min_max(rect.min, filled_rect.max);
		// painter.rect_filled(remaining_rect, 0.0, Color32::from_rgb(200, 0, 0));
		let color_saturation = if self.mute || (!self.solo && self.others_solo) { 50 } else { 200 };
		let color = if val < 1.0 { 
			Color32::from_rgb(0, color_saturation, 0) 
		} else if val < self.max_gain {
			Color32::from_rgb(color_saturation, color_saturation, 0)
		} else { 
			Color32::from_rgb(color_saturation, 0, 0) 
		};
		painter.rect_filled(filled_rect, 0.0, color); 
		painter.rect_stroke(rect, 0.0, (1.0, Color32::DARK_GRAY));
		// Draw scale numbers 
		let num_steps = (self.max_gain * 10.0) as u16;
		let step_size = rect.height() / num_steps as f32; 
		for i in 0..=num_steps { 
			let y_pos = rect.top() + i as f32 * step_size; 
			let number = if self.show_rms { 
				db_rms((num_steps - i) as f32 / 10.0)
			} else { 
				db_peak((num_steps - i) as f32 / 10.0)
			}; 
			// Invert the order if you want 0 at the bottom 
			let text_pos = Pos2::new(rect.right() + 5.0, y_pos);
			painter.text(text_pos, 
				Align2::LEFT_CENTER, 
				format!("{:.1}", number), 
				FontId::new(9.0, FontFamily::Monospace),
				Color32::DARK_GRAY);
		}
		response.on_hover_cursor(egui::CursorIcon::PointingHand) 
			.on_hover_text(format!("{:.3} dB", val_db)); 
	}

	fn declare_jack_port(&self, client : &jack::Client) -> jack::Port<jack::AudioIn> {
		client.register_port(&self.channel_name, jack::AudioIn::default()).unwrap()
	}

	fn rms(&mut self, input : &[f32]) {
		self.last_rms = (input.iter().map(|x| x * x).sum::<f32>() / input.len() as f32).sqrt();
	}

	fn update_smoothed(&mut self, peak : f32) {
		if peak > self.last_smoothed {
			self.last_smoothed = peak;
			self.peak_hold_counter = PEAK_HOLD_TIME;
		} else if self.peak_hold_counter > 0 {
			self.peak_hold_counter -= 1;
		} else {
			self.last_smoothed *= DECAY_FACTOR;
		}
	}
}

impl Default for MixChannel {
	fn default() -> Self {
	   Self {
			gain: 1.0,
			last: 0.0,
			last_smoothed: 0.0,
			peak_hold_counter: 0,
			max: 0.0,
			last_rms: 0.0,
			channel_name: "Channel".to_owned(),
			limit: false,
			mute: false,
			solo: false,
			others_solo: false,
			show_rms: false,
			max_gain: 1.25,
		}
	}
}
