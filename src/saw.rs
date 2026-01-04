use std::usize;

use ffmpeg_next::{self as ffmpeg, format, media::Type, util::range::Range, Error};

enum Direction {
    Forward,
    Backward,
}
struct Fragment {
    start: i64,
    end: i64,
}
impl Range<i64> for Fragment {
    fn start(&self) -> Option<&i64> {
        Some(&self.start)
    }

    fn end(&self) -> Option<&i64> {
        Some(&self.end)
    }
}

pub struct Saw {
    ictx: format::context::Input,
    octx: format::context::Output,
    stream_map: Vec<usize>,
    pub start: f64,
    pub first_kf: Option<f64>,
    pub last_kf: Option<f64>,
    end: f64,
}

impl std::fmt::Debug for Saw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Streams: \n")?;
        for stream in self.ictx.streams() {
            let info = format!("{:?}", stream.parameters().medium());
            let id = format!("{:?}", stream.parameters().id());
            write!(f, "\nType: {{")?;
            write!(f, " {}", info)?;
            write!(f, "  {} ", id)?;
            write!(f, "}}\n")?;
        }
        // Write raw multiline string
        writeln!(f, "  start: {:?},", &self.start)?;
        writeln!(f, "  first_kf: {:?},", &self.first_kf)?;
        writeln!(f, "  last_kf: {:?},", &self.last_kf)?;
        writeln!(f, "  end: {:?},", &self.end)
    }
}

impl Saw {
    pub fn new(input: &str, output: &str, start: f64, end: f64) -> Result<Self, Error> {
        ffmpeg::init()?;
        let ictx = format::input(&input)?;
        let mut octx = format::output(&output)?;
        let mut stream_map = Vec::with_capacity(ictx.nb_streams() as usize);
        for istream in ictx.streams() {
            let codec_id = istream.parameters().id();
            let codec = ffmpeg::codec::decoder::find(codec_id).take();
            let mut ostream = octx.add_stream(codec)?;
            ostream.set_parameters(istream.parameters());
            stream_map.insert(istream.index() as usize, ostream.index() as usize);
        }
        octx.write_header()?;
        Ok(Saw {
            ictx,
            octx,
            stream_map,
            start,
            first_kf: None,
            last_kf: None,
            end,
        })
    }

    pub fn seek(&mut self) -> Result<(), Error> {
        self.first_kf =
            self.find_closest_keyframe_inside_boundaries(self.start, Direction::Forward)?;
        self.last_kf =
            self.find_closest_keyframe_inside_boundaries(self.end, Direction::Backward)?;
        Ok(())
    }

    fn find_closest_keyframe_inside_boundaries(
        &mut self,
        target_time_seconds: f64,
        direction: Direction,
    ) -> Result<Option<f64>, Error> {
        let stream = self
            .ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let time_base = stream.time_base();
        let stream_index = stream.index();

        // Convert target time to stream time base units
        let target_ts = (target_time_seconds / f64::from(time_base)) as i64;

        let direction = match direction {
            Direction::Forward => ffmpeg::ffi::AVSEEK_FLAG_FRAME,
            Direction::Backward => ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
        };
        // Seek to nearest keyframe BEFORE or AT target_ts
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.ictx.as_mut_ptr(),
                stream_index as i32,
                target_ts,
                direction,
            );
        }

        // Read packets forward until we find the first keyframe
        for (stream, packet) in self.ictx.packets() {
            if stream.index() != stream_index {
                continue;
            }

            if packet.is_key() {
                let ts = packet.pts().or_else(|| packet.dts()).unwrap();
                let keyframe_time = (ts as f64) * f64::from(time_base);
                return Ok(Some(keyframe_time));
            }
        }

        unsafe {
            ffmpeg::ffi::avformat_flush(self.ictx.as_mut_ptr());
        }
        Ok(None)
    }

    // pub fn copy_between_keyframes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    //     if self.first_kf.is_none() || self.last_kf.is_none() {
    //         panic!("No first or last keyframe found");
    //     }
    //     let fragment = Fragment {
    //         start: self.first_kf.unwrap() as i64,
    //         end: self.last_kf.unwrap() as i64,
    //     };
    //     self.ictx
    //         .seek(self.first_kf.unwrap() as i64, fragment)
    //         .expect("Failed to seek");
    //
    //     let video_stream_index = self
    //         .ictx
    //         .streams()
    //         .best(Type::Video)
    //         .ok_or(ffmpeg::Error::StreamNotFound)?
    //         .index();
    //
    //     // Read packets forward until we find the first keyframe
    //     for (stream, mut packet) in self.ictx.packets() {
    //         if stream.index() != video_stream_index {
    //             continue;
    //         }
    //
    //         packet.set_stream(video_stream_index);
    //         self.octx.stream(video_stream_index);
    //     }
    //     Ok(())
    // }

    pub fn copy_packets_between_keyframes(&mut self) -> Result<(), ffmpeg::Error> {
        // Берём time_base видео как опорный
        let video_stream = self.ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let vindex = video_stream.index();
        let v_tb = video_stream.time_base();

        let start = self.first_kf.unwrap();
        let end = self.last_kf.unwrap();

        let start_ts = (start / f64::from(v_tb)) as i64;
        let end_ts = (end / f64::from(v_tb)) as i64;

        // Seek строго к keyframe ≤ start
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.ictx.as_mut_ptr(),
                vindex as i32,
                start_ts,
                ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
            );
        }

        // Flush после seek
        unsafe {
            ffmpeg::ffi::avformat_flush(self.ictx.as_mut_ptr());
        }

        // Запоминаем стартовые DTS для каждого стрима
        let mut first_dts: Vec<Option<i64>> = vec![None; self.ictx.streams().len()];

        for (stream, mut packet) in self.ictx.packets() {
            let istream_index = stream.index();

            // Стримы, которых нет в output — пропускаем
            if istream_index >= self.stream_map.len() {
                continue;
            }

            let tb = stream.time_base();
            let pts = packet.pts().unwrap_or(0);
            let time = pts as f64 * f64::from(tb);

            if time < start {
                continue;
            }
            if time > end {
                break;
            }

            // Инициализация first_dts
            let base = first_dts[istream_index].get_or_insert(packet.dts().unwrap_or(0));

            // Сдвигаем timestamps
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts - *base));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts - *base));
            }

            // Remap stream index
            packet.set_stream(self.stream_map[istream_index]);
            packet.write_interleaved(&mut self.octx).unwrap();
        }

        Ok(())
    }

    fn reencode_between_timestamps(&mut self) {
    println!("reencode_between_timestamps");

}

    pub fn saw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.first_kf.is_some() {
            self.reencode_between_timestamps();
        }
        if self.first_kf.is_some() && self.last_kf.is_some() {
            self.copy_packets_between_keyframes()?;
        }
        if self.last_kf.is_some() {
            self.reencode_between_timestamps();
        }
        self.octx.write_trailer()?;
        Ok(())
    }
}
