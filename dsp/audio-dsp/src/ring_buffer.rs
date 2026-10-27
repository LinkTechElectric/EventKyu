/// Preallocated interleaved multichannel ring buffer for input-to-output
/// passthrough.
///
/// Reads and writes require `&mut self`, giving single-producer/single-consumer
/// semantics within one audio callback or owner. It intentionally contains no
/// cross-thread lock-free synchronization; a HAL that shares it between threads
/// must provide an appropriate external wrapper. Writes never overwrite unread
/// frames. Short writes count as overruns, while missing read frames are zeroed
/// and counted as underruns.
pub struct InterleavedRingBuffer {
    channels: usize,
    capacity_frames: usize,
    buffer: Vec<f32>,
    read_frame: usize,
    write_frame: usize,
    len_frames: usize,
    underrun_frames: u64,
    overrun_frames: u64,
}

impl InterleavedRingBuffer {
    /// Allocates storage for `capacity_frames` frames on `channels` channels.
    ///
    /// # Panics
    ///
    /// Panics if either argument is zero.
    pub fn new(channels: usize, capacity_frames: usize) -> Self {
        assert!(channels > 0, "channels must be non-zero");
        assert!(capacity_frames > 0, "capacity_frames must be non-zero");
        Self {
            channels,
            capacity_frames,
            buffer: vec![0.0; channels * capacity_frames],
            read_frame: 0,
            write_frame: 0,
            len_frames: 0,
            underrun_frames: 0,
            overrun_frames: 0,
        }
    }

    /// Writes as many whole frames as fit and returns that frame count.
    ///
    /// Frames that do not fit are dropped and added to the overrun counter.
    ///
    /// # Panics
    ///
    /// Panics if `input` does not contain a whole number of frames.
    pub fn write(&mut self, input: &[f32]) -> usize {
        assert_eq!(
            input.len() % self.channels,
            0,
            "input must contain whole frames"
        );
        let offered_frames = input.len() / self.channels;
        let written_frames = offered_frames.min(self.capacity_frames - self.len_frames);

        for frame in input[..written_frames * self.channels].chunks_exact(self.channels) {
            let destination = self.write_frame * self.channels;
            self.buffer[destination..destination + self.channels].copy_from_slice(frame);
            self.write_frame = (self.write_frame + 1) % self.capacity_frames;
        }
        self.len_frames += written_frames;
        self.overrun_frames = self
            .overrun_frames
            .saturating_add((offered_frames - written_frames) as u64);
        written_frames
    }

    /// Reads whole frames into `output` and returns the available frame count.
    ///
    /// If fewer frames are available, the rest of `output` is filled with
    /// silence and the missing frames are added to the underrun counter.
    ///
    /// # Panics
    ///
    /// Panics if `output` does not contain a whole number of frames.
    pub fn read(&mut self, output: &mut [f32]) -> usize {
        assert_eq!(
            output.len() % self.channels,
            0,
            "output must contain whole frames"
        );
        let requested_frames = output.len() / self.channels;
        let read_frames = requested_frames.min(self.len_frames);

        for frame in output[..read_frames * self.channels].chunks_exact_mut(self.channels) {
            let source = self.read_frame * self.channels;
            frame.copy_from_slice(&self.buffer[source..source + self.channels]);
            self.read_frame = (self.read_frame + 1) % self.capacity_frames;
        }
        output[read_frames * self.channels..].fill(0.0);
        self.len_frames -= read_frames;
        self.underrun_frames = self
            .underrun_frames
            .saturating_add((requested_frames - read_frames) as u64);
        read_frames
    }

    /// Returns the number of readable frames.
    pub fn len(&self) -> usize {
        self.len_frames
    }

    /// Returns whether no frames are available to read.
    pub fn is_empty(&self) -> bool {
        self.len_frames == 0
    }

    /// Returns the number of frames that can be written without overrun.
    pub fn available_write(&self) -> usize {
        self.capacity_frames - self.len_frames
    }

    /// Returns the capacity in frames.
    pub fn capacity(&self) -> usize {
        self.capacity_frames
    }

    /// Returns the total number of frames missing from reads since reset.
    pub fn underrun_frames(&self) -> u64 {
        self.underrun_frames
    }

    /// Returns the total number of frames dropped from writes since reset.
    pub fn overrun_frames(&self) -> u64 {
        self.overrun_frames
    }

    /// Drops buffered frames and clears both monitoring counters.
    pub fn reset(&mut self) {
        self.read_frame = 0;
        self.write_frame = 0;
        self.len_frames = 0;
        self.underrun_frames = 0;
        self.overrun_frames = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::InterleavedRingBuffer;

    #[test]
    fn stereo_buffer_wraps_without_reordering_frames() {
        let mut ring = InterleavedRingBuffer::new(2, 3);
        assert_eq!(ring.write(&[1.0, 10.0, 2.0, 20.0]), 2);
        let mut first = [0.0; 2];
        assert_eq!(ring.read(&mut first), 1);
        assert_eq!(first, [1.0, 10.0]);

        assert_eq!(ring.write(&[3.0, 30.0, 4.0, 40.0]), 2);
        let mut output = [0.0; 6];
        assert_eq!(ring.read(&mut output), 3);
        assert_eq!(output, [2.0, 20.0, 3.0, 30.0, 4.0, 40.0]);
        assert!(ring.is_empty());
    }

    #[test]
    fn underrun_zero_fills_output_and_counts_missing_frames() {
        let mut ring = InterleavedRingBuffer::new(1, 3);
        ring.write(&[7.0]);
        let mut output = [9.0; 3];
        assert_eq!(ring.read(&mut output), 1);
        assert_eq!(output, [7.0, 0.0, 0.0]);
        assert_eq!(ring.underrun_frames(), 2);
    }

    #[test]
    fn overrun_does_not_overwrite_unread_audio() {
        let mut ring = InterleavedRingBuffer::new(1, 2);
        assert_eq!(ring.write(&[1.0, 2.0, 3.0]), 2);
        assert_eq!(ring.overrun_frames(), 1);
        assert_eq!(ring.available_write(), 0);
        let mut output = [0.0; 2];
        assert_eq!(ring.read(&mut output), 2);
        assert_eq!(output, [1.0, 2.0]);
    }

    #[test]
    fn reset_clears_audio_state_and_counters() {
        let mut ring = InterleavedRingBuffer::new(1, 1);
        ring.write(&[1.0, 2.0]);
        let mut output = [0.0; 2];
        ring.read(&mut output);
        ring.reset();

        assert_eq!(ring.len(), 0);
        assert_eq!(ring.available_write(), 1);
        assert_eq!(ring.underrun_frames(), 0);
        assert_eq!(ring.overrun_frames(), 0);
    }
}
