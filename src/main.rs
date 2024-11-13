#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::*;

fn main() -> eframe::Result {
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

			Ok(Box::<FslcMix>::new(FslcMix::new(10)))
		}),
	)
}

struct FslcMix {
	channels: Vec<MixChannel>,
	master: MixChannel,
}

impl FslcMix {
	fn new(num_channels: u8) -> Self {
		Self { 
			channels: (0..num_channels).map(|_| MixChannel::default()).collect(),
			master: MixChannel { channel_name: "Master".to_owned(), ..Default::default() }
		}
	}
}

impl eframe::App for FslcMix {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
	    egui::Window::new("Mixer (FSLCMix)")
			.default_pos([100.0, 100.0])
			.title_bar(true)
			.show(ctx, |ui|{
				ui.vertical(|ui| {
					ui.label("Licensed under the GPLv3.");
				});
				ui.horizontal(|ui| {
					self.master.ui(ui);
					ui.separator();
					for channel in &mut self.channels {
						channel.ui(ui);
					}
				});
			});
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
	fn mix(&mut self, input : &[f32], output : &mut [f32]) {
		// Sanity check
		assert!(input.len() == output.len());
		for i in 0..input.len() {
			let sample = input[i] * self.gain;
			if self.limit && sample >= 1.0 {
				output[i] = 1.0;
			} else {
				output[i] = sample;
			}
			if output[i] > self.max {
				self.max = output[i];
			}
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
