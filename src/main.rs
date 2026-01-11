use clap::Parser;
mod reencoder;
mod saw;

use reencoder::Transcoder;
use reencoder::DEFAULT_X264_OPTS;
use std::collections::HashMap;

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
    let x264_opts = reencoder::parse_opts(
            DEFAULT_X264_OPTS.to_string()
    )
    .expect("invalid x264 options string");

    eprintln!("x264 options: {:?}", x264_opts);

    ffmpeg::init().unwrap();
    log::set_level(log::Level::Info);

    let mut ictx = format::input(&input_file).unwrap();
    let mut octx = format::output(&output_file).unwrap();

    format::context::input::dump(&ictx, 0, Some(&input_file));

    let best_video_stream_index = ictx
        .streams()
        .best(media::Type::Video)
        .map(|stream| stream.index());
    let mut stream_mapping: Vec<isize> = vec![0; ictx.nb_streams() as _];
    let mut ist_time_bases = vec![Rational(0, 0); ictx.nb_streams() as _];
    let mut ost_time_bases = vec![Rational(0, 0); ictx.nb_streams() as _];
    let mut transcoders = HashMap::new();
    let mut ost_index = 0;
    for (ist_index, ist) in ictx.streams().enumerate() {
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
                    &mut octx,
                    ost_index as _,
                    x264_opts.to_owned(),
                    Some(ist_index) == best_video_stream_index,
                )
                .unwrap(),
            );
        } else {
            // Set up for stream copy for non-video stream.
            let mut ost = octx.add_stream(encoder::find(codec::Id::None)).unwrap();
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

    octx.set_metadata(ictx.metadata().to_owned());
    format::context::output::dump(&octx, 0, Some(&output_file));
    octx.write_header().unwrap();

    for (ost_index, _) in octx.streams().enumerate() {
        ost_time_bases[ost_index] = octx.stream(ost_index as _).unwrap().time_base();
    }

    for (stream, mut packet) in ictx.packets() {
        let ist_index = stream.index();
        let ost_index = stream_mapping[ist_index];
        if ost_index < 0 {
            continue;
        }
        let ost_time_base = ost_time_bases[ost_index as usize];
        match transcoders.get_mut(&ist_index) {
            Some(transcoder) => {
                transcoder.send_packet_to_decoder(&packet);
                transcoder.receive_and_process_decoded_frames(&mut octx, ost_time_base);
            }
            None => {
                // Do stream copy on non-video streams.
                packet.rescale_ts(ist_time_bases[ist_index], ost_time_base);
                packet.set_position(-1);
                packet.set_stream(ost_index as _);
                packet.write_interleaved(&mut octx).unwrap();
            }
        }
    }

    // Flush encoders and decoders.
    for (ost_index, transcoder) in transcoders.iter_mut() {
        let ost_time_base = ost_time_bases[*ost_index];
        transcoder.send_eof_to_decoder();
        transcoder.receive_and_process_decoded_frames(&mut octx, ost_time_base);
        transcoder.send_eof_to_encoder();
        transcoder.receive_and_process_encoded_packets(&mut octx, ost_time_base);
    }

    octx.write_trailer().unwrap();
    Ok(())
}
