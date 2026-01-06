use ffmpeg_next::{self as ffmpeg};

pub(crate) struct VideoCodec {
    pub decoder: ffmpeg::decoder::Video,
    pub encoder: ffmpeg::encoder::video::Video,
    pub in_time_base: ffmpeg::Rational,
    pub out_time_base: ffmpeg::Rational,
}

pub(crate) struct AudioCodec {
    pub decoder: ffmpeg::decoder::Audio,
    pub encoder: ffmpeg::encoder::audio::Audio,
    pub in_time_base: ffmpeg::Rational,
    pub out_time_base: ffmpeg::Rational,
}
pub(crate) enum StreamCodec {
    Video(VideoCodec),
    Audio(AudioCodec),
    Other, // subtitles / data
}
