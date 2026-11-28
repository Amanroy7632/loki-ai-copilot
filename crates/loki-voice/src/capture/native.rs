//! CPAL's cross-platform default input. OS permission checks remain entirely with CPAL/the OS.

use super::{AudioSource, CaptureError, CaptureSink, InputFormat};
use cpal::{
    SampleFormat,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

/// The default device and its native configuration. Constructing this does not start capture.
pub struct CpalSource {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
}

impl CpalSource {
    /// Finds the default microphone; errors are explicit and never trigger a fallback device.
    pub fn default_input() -> Result<Self, CaptureError> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(CaptureError::NoInputDevice)?;
        let config = device.default_input_config().map_err(map_error)?;
        Ok(Self { device, config })
    }

    fn build<T>(&self, sink: CaptureSink) -> Result<cpal::Stream, CaptureError>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let errors = sink.clone();
        self.device
            .build_input_stream(
                self.config.config(),
                move |data: &[T], _| sink.push_interleaved(data),
                move |error| errors.fail(map_error(error)),
                None,
            )
            .map_err(map_error)
    }
}

impl AudioSource for CpalSource {
    type Stream = cpal::Stream;

    fn format(&self) -> InputFormat {
        InputFormat {
            sample_rate: self.config.sample_rate(),
            channels: self.config.channels(),
        }
    }

    fn start(self, sink: CaptureSink) -> Result<Self::Stream, CaptureError> {
        let stream = match self.config.sample_format() {
            SampleFormat::I8 => self.build::<i8>(sink),
            SampleFormat::I16 => self.build::<i16>(sink),
            SampleFormat::I24 => self.build::<cpal::I24>(sink),
            SampleFormat::I32 => self.build::<i32>(sink),
            SampleFormat::I64 => self.build::<i64>(sink),
            SampleFormat::U8 => self.build::<u8>(sink),
            SampleFormat::U16 => self.build::<u16>(sink),
            SampleFormat::U24 => self.build::<cpal::U24>(sink),
            SampleFormat::U32 => self.build::<u32>(sink),
            SampleFormat::U64 => self.build::<u64>(sink),
            SampleFormat::F32 => self.build::<f32>(sink),
            SampleFormat::F64 => self.build::<f64>(sink),
            _ => Err(CaptureError::UnsupportedFormat),
        }?;
        stream.play().map_err(map_error)?;
        Ok(stream)
    }
}

fn map_error(error: cpal::Error) -> CaptureError {
    match error.kind() {
        cpal::ErrorKind::PermissionDenied => CaptureError::PermissionDenied,
        cpal::ErrorKind::DeviceNotAvailable
        | cpal::ErrorKind::StreamInvalidated
        | cpal::ErrorKind::DeviceChanged => CaptureError::DeviceLost,
        cpal::ErrorKind::UnsupportedConfig | cpal::ErrorKind::UnsupportedOperation => {
            CaptureError::UnsupportedFormat
        }
        _ => CaptureError::Backend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_errors_preserve_permission_denial_and_device_loss() {
        for (kind, expected) in [
            (
                cpal::ErrorKind::PermissionDenied,
                CaptureError::PermissionDenied,
            ),
            (
                cpal::ErrorKind::DeviceNotAvailable,
                CaptureError::DeviceLost,
            ),
            (cpal::ErrorKind::StreamInvalidated, CaptureError::DeviceLost),
            (cpal::ErrorKind::DeviceChanged, CaptureError::DeviceLost),
            (
                cpal::ErrorKind::UnsupportedConfig,
                CaptureError::UnsupportedFormat,
            ),
            (cpal::ErrorKind::BackendError, CaptureError::Backend),
        ] {
            assert_eq!(map_error(kind.into()), expected);
        }
    }
}
