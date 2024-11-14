#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use eframe::egui::*;

fn main() -> eframe::Result {
	let args = Args::parse();
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default(),
		..Default::default()
	};
	eframe::run_native(
		"FSLCMix", 
		options,
		Box::new(|cc| {
			// Dark theme
			cc.egui_ctx.set_theme(egui::Theme::Dark);
			// egui_extras::install_image_loaders(&cc.egui_ctx);

			Ok(Box::<FslcMix>::new(FslcMix::new(args.channels)))
		}),
	)
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
			ui_size: egui::Vec2::new(400.0, 200.0), // This size doesn't matter since it's
													// overritten
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

		}
		self.master.last = output[output.len() - 1];
	}
}

impl eframe::App for FslcMix {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
	    //egui::Window::new("Mixer (FSLCMix)")
		//	.default_pos([100.0, 100.0])
		//	.title_bar(true)
		egui::CentralPanel::default()
			.show(ctx, |ui|{
				ui.vertical(|ui| {
					ui.horizontal(|ui| {
						// ui.label("Licensed under the GPLv3.");
						ui.toggle_value(&mut self.normalize, "Normalize")
					});
				});
				ui.horizontal(|ui| {
					self.master.ui(ui);
					ui.separator();
					for channel in &mut self.channels {
						channel.ui(ui);
					}
				});
				self.ui_size = ui.min_size();
			});
		let window_size = self.ui_size + egui::vec2(20.0, 40.0);
		// frame.set_window_size(window_size);
	}
}

struct MixChannel {
	gain: f32,
	last: f32,
	max: f32,
	channel_name: String,
	limit: bool,
	mute: bool,
	solo: bool,
}

impl MixChannel {

	fn mix(&mut self, input : &[f32], output : &mut [f32], any_solo : bool) {
		if self.mute || (any_solo && !self.solo) {
			self.last = 0.0;
			return;
		}
		// Sanity check
		assert!(input.len() == output.len());
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
			output[i] += out_sample;
		}
		self.last = output[output.len() - 1];
	}

	fn ui(&mut self, ui : &mut egui::Ui) {
		ui.vertical(|ui| {
			ui.horizontal(|ui| {
				ui.label(format!("Peak: {} dB", self.max.log10()));
			});
			ui.horizontal(|ui| {
				ui.add(egui::Slider::new(&mut self.gain, 0.0..=1.2).text("Gain").vertical());
				self.levels_bar(ui, self.last);
			});

			let btn = ui.button("Reset Gain");
			if btn.clicked() {
				self.gain = 1.0;
			}
			ui.horizontal(|ui| {
				ui.toggle_value(&mut self.mute, "M");
				ui.toggle_value(&mut self.solo, "S");
				ui.toggle_value(&mut self.limit, "Limit");
			});
			ui.add(egui::TextEdit::singleline(&mut self.channel_name).desired_width(75.0));
		});
	}

	fn levels_bar(&self, ui: &mut Ui, value: f32) { 
		let (rect, response) = ui.allocate_exact_size(vec2(20.0, 200.0), egui::Sense::hover()); 
		let painter = ui.painter(); 
		let filled_height = rect.height() * value / 1.2; // Show a bit over max amplitude 
		let filled_rect = Rect::from_min_max(rect.min, rect.min + vec2(rect.width(), filled_height)); 
		let remaining_rect = Rect::from_min_max(filled_rect.max, rect.max); 
		painter.rect_filled(filled_rect, 0.0, Color32::from_rgb(0, 200, 0)); 
painter.rect_filled(remaining_rect, 0.0, Color32::from_rgb(200, 0, 0)); 
		painter.rect_stroke(rect, 0.0, (1.0, Color32::WHITE)); 
		response.on_hover_cursor(egui::CursorIcon::PointingHand) 
			.on_hover_text(format!("{:.1} db", value.log10())); 
	}
}

impl Default for MixChannel {
	fn default() -> Self {
	    Self {
			gain: 1.0,
			last: 0.0,
			max: 0.0,
			channel_name: "Channel".to_owned(),
			limit: false,
			mute: false,
			solo: false,
		}
	}
}
