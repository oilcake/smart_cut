use clap::Parser;
mod reencoder;
mod saw;

use reencoder::Transcoder;
use reencoder::DEFAULT_X264_OPTS;
use std::collections::HashMap;
use std::env::args;

use ffmpeg::{codec, encoder, format, log, media, Rational};
use ffmpeg_next as ffmpeg;

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "smart_cut")]
#[command(author = "oilcake")]
#[command(version = "0.1")]
#[command(about = "Almost lossless video cutter", long_about = None)]
pub struct Args {
    /// Input video file path
    #[arg(short, long)]
    pub input: String,

    #[arg(short, long)]
    pub output: String,

    /// Start time in seconds
    #[arg(long)]
    pub start: f64,

    /// End time in seconds
    #[arg(long)]
    pub end: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    //
    //     let mut saw = saw::Saw::new(&args.input, &args.output, args.start, args.end).unwrap();
    //     saw.seek()?;
    //
    //     dbg!(&saw);
    //
    //     saw.saw()?;
    //
    //     Ok(())
    // }
    let input_file = &args.input;
    let output_file = &args.output;

    ffmpeg::init().unwrap();
    log::set_level(log::Level::Info);

    let ictx = format::input(&input_file).unwrap();
    let octx = format::output(&output_file).unwrap();

    format::context::input::dump(&ictx, 0, Some(&input_file));

    let mut saw2 = Saw2::new(ictx, octx);

    saw2.reencode_between_timestamps(args.start, args.end)?;

    saw2.finalize();
    Ok(())
}

struct Saw2 {
    ictx: ffmpeg::format::context::Input,
    octx: ffmpeg::format::context::Output,
}

impl Saw2 {
    fn new(ictx: ffmpeg::format::context::Input, octx: ffmpeg::format::context::Output) -> Self {
        Saw2 { ictx, octx }
    }
    fn finalize(&mut self) {
        self.octx.write_trailer().unwrap();
    }

    fn reencode_between_timestamps(
        &mut self,
        start: f64,
        end: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let best_video_stream_index = self
            .ictx
            .streams()
            .best(media::Type::Video)
            .map(|stream| stream.index());
        let mut stream_mapping: Vec<isize> = vec![0; self.ictx.nb_streams() as _];
        let mut ist_time_bases = vec![Rational(0, 0); self.ictx.nb_streams() as _];
        let mut ost_time_bases = vec![Rational(0, 0); self.ictx.nb_streams() as _];
        let mut transcoders = HashMap::new();
        let mut ost_index = 0;
        for (ist_index, ist) in self.ictx.streams().enumerate() {
            let ist_medium = ist.parameters().medium();
            if ist_medium != media::Type::Audio
                && ist_medium != media::Type::Video
                && ist_medium != media::Type::Subtitle
            {
                stream_mapping[ist_index] = -1;
                continue;
            }
            stream_mapping[ist_index] = ost_index;
            ist_time_bases[ist_index] = ist.time_base();
            if ist_medium == media::Type::Video {
                // Initialize transcoder for video stream.
                transcoders.insert(
                    ist_index,
                    Transcoder::new(
                        &ist,
                        &mut self.octx,
                        ost_index as _,
                        // x264_opts.to_owned(),
                        Some(ist_index) == best_video_stream_index,
                    )
                    .unwrap(),
                );
            } else {
                // Set up for stream copy for non-video stream.
                let mut ost = self
                    .octx
                    .add_stream(encoder::find(codec::Id::None))
                    .unwrap();
                ost.set_parameters(ist.parameters());
                // We need to set codec_tag to 0 lest we run into incompatible codec tag
                // issues when muxing into a different container format. Unfortunately
                // there's no high level API to do this (yet).
                unsafe {
                    (*ost.parameters().as_mut_ptr()).codec_tag = 0;
                }
            }
            ost_index += 1;
        }

        self.octx.set_metadata(self.ictx.metadata().to_owned());
        // format::context::output::dump(&octx, 0, Some(&output_file));
        self.octx.write_header().unwrap();

        for (ost_index, _) in self.octx.streams().enumerate() {
            ost_time_bases[ost_index] = self.octx.stream(ost_index as _).unwrap().time_base();
        }

        // let fragment = Fragment {
        //     start: self.first_kf.unwrap() as _,
        //     end: self.last_kf.unwrap() as _,
        // };
        // println!("Fragment {:?}", fragment);
        self.ictx
            .seek(start as i64, ..(start as i64))
            .expect("Failed to seek");

        let mut first_dts: Vec<Option<i64>> = vec![None; self.ictx.streams().len()];
        for (stream, mut packet) in self.ictx.packets() {
            let ist_index = stream.index();
            let ost_index = stream_mapping[ist_index];
            if ost_index < 0 {
                continue;
            }
            let tb = stream.time_base();

            let pts = packet
                .pts()
                .or_else(|| packet.dts())
                .ok_or(ffmpeg::Error::InvalidData)?;

            let time = pts as f64 * f64::from(tb);

            if time < start {
                continue;
            }
            if time > end {
                break;
            }

            // Инициализация first_dts
            let base = first_dts[ist_index].get_or_insert(packet.dts().unwrap_or(0));

            // Сдвигаем timestamps
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts - *base));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts - *base));
            }
            let ost_time_base = ost_time_bases[ost_index as usize];
            match transcoders.get_mut(&ist_index) {
                Some(transcoder) => {
                    transcoder.send_packet_to_decoder(&packet);
                    transcoder.receive_and_process_decoded_frames(&mut self.octx, ost_time_base);
                }
                None => {
                    // Do stream copy on non-video streams.
                    packet.rescale_ts(ist_time_bases[ist_index], ost_time_base);
                    packet.set_position(-1);
                    packet.set_stream(ost_index as _);
                    packet.write_interleaved(&mut self.octx).unwrap();
                }
            }
        }

        // Flush encoders and decoders.
        for (ost_index, transcoder) in transcoders.iter_mut() {
            let ost_time_base = ost_time_bases[*ost_index];
            transcoder.send_eof_to_decoder();
            transcoder.receive_and_process_decoded_frames(&mut self.octx, ost_time_base);
            transcoder.send_eof_to_encoder();
            transcoder.receive_and_process_encoded_packets(&mut self.octx, ost_time_base);
        }
        Ok(())
    }
}
