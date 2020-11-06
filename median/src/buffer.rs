//! Data access to MSP buffer~ object data.
use crate::{notify::Notification, symbol::SymbolRef};
use core::ffi::c_void;
use std::convert::TryFrom;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

lazy_static::lazy_static! {
    static ref GLOBAL_SYMBOL_BINDING: SymbolRef = SymbolRef::try_from("globalsymbol_binding").unwrap();
    static ref GLOBAL_SYMBOL_UNBINDING: SymbolRef = SymbolRef::try_from("globalsymbol_unbinding").unwrap();
    static ref GET_NAME: SymbolRef = SymbolRef::try_from("getname").unwrap();
}

/// A safe wrapper for `max_sys::t_buffer_ref` objects.
pub struct BufferRef {
    value: *mut max_sys::t_buffer_ref,
    buffer_name: SymbolRef,
}

/// A locked buffer, for sample data access.
pub struct BufferLocked {
    buffer: *mut max_sys::t_buffer_obj,
    samples: *mut f32,
    dirty: bool,
}

struct BufferChannelIter<'a> {
    samples: *mut f32,
    frames: usize,
    channels: usize,
    offset: usize,
    end: usize,
    _phantom: PhantomData<&'a ()>,
}

struct BufferChannelIterMut<'a> {
    samples: *mut f32,
    frames: usize,
    channels: usize,
    offset: usize,
    end: usize,
    _phantom: PhantomData<&'a ()>,
}

impl BufferRef {
    /// Create a new buffer reference.
    ///
    /// # Remarks
    /// * You must have a notify method in your owner.
    pub unsafe fn new(owner: *mut max_sys::t_object, name: Option<SymbolRef>) -> Self {
        let name = name.unwrap_or_else(|| crate::max::common_symbols().s_nothing.into());
        Self {
            value: max_sys::buffer_ref_new(owner, name.inner()),
            buffer_name: name,
        }
    }

    pub fn set(&mut self, name: SymbolRef) {
        unsafe {
            self.buffer_name = name;
            max_sys::buffer_ref_set(self.value, self.buffer_name.inner());
        }
    }

    pub fn exists(&self) -> bool {
        unsafe { max_sys::buffer_ref_exists(self.value) != 0 }
    }

    fn buffer(&self) -> Option<*mut max_sys::t_buffer_obj> {
        unsafe {
            let buffer = max_sys::buffer_ref_getobject(self.value);
            if buffer.is_null() {
                None
            } else {
                Some(buffer)
            }
        }
    }

    /// Get the number of channels that the referenced buffer has, if there is a buffer.
    pub fn channels(&self) -> Option<usize> {
        self.buffer()
            .map(|buffer| unsafe { max_sys::buffer_getchannelcount(buffer) as _ })
    }

    /// Get the number of frames that the referenced buffer has, if there is a buffer.
    pub fn frames(&self) -> Option<usize> {
        self.buffer()
            .map(|buffer| unsafe { max_sys::buffer_getframecount(buffer) as _ })
    }

    /// Get the sample rate, samples per second, of referenced buffer data, if there is a buffer.
    pub fn sample_rate(&self) -> Option<f64> {
        self.buffer()
            .map(|buffer| unsafe { max_sys::buffer_getsamplerate(buffer) })
    }

    /// Get the sample rate, samples per milliseconds, of referenced buffer data, if there is a buffer.
    pub fn millisample_rate(&self) -> Option<f64> {
        self.buffer()
            .map(|buffer| unsafe { max_sys::buffer_getmillisamplerate(buffer) })
    }

    /// Lock the buffer if it exists.
    pub fn lock(&mut self) -> Option<BufferLocked> {
        unsafe {
            let buffer = max_sys::buffer_ref_getobject(self.value);
            if buffer.is_null() {
                None
            } else {
                let samples = max_sys::buffer_locksamples(buffer);
                if samples.is_null() {
                    None
                } else {
                    Some(BufferLocked {
                        buffer,
                        samples,
                        dirty: false,
                    })
                }
            }
        }
    }

    /// Apply the notification to this buffer reference it if its applicable.
    ///
    /// # Remarks
    /// It should be okay to send notifications that are intended for other objects, including
    /// other buffer references.
    pub fn notify_if(&mut self, notification: &Notification) {
        let sender = notification.sender();
        if !sender.is_null() {
            let message = notification.message();
            //see if it is a binding or unbinding message
            if *message == *GLOBAL_SYMBOL_BINDING || *message == *GLOBAL_SYMBOL_UNBINDING {
                unsafe {
                    //try to get the name of the buffer
                    let name: *mut max_sys::t_symbol = std::ptr::null_mut();
                    max_sys::object_method(
                        notification.data(),
                        GET_NAME.inner(),
                        std::mem::transmute::<_, *mut c_void>(&name),
                    );
                    //if the name matches our buffer's name, send notification
                    if !name.is_null() && SymbolRef::from(name) == self.buffer_name {
                        max_sys::buffer_ref_notify(
                            self.value,
                            notification.sender_name().inner(),
                            message.inner(),
                            sender,
                            notification.data(),
                        );
                    }
                }
            }
        }
    }
}

impl BufferLocked {
    /// Get the number of channels that the buffer has.
    pub fn channels(&self) -> usize {
        unsafe { max_sys::buffer_getchannelcount(self.buffer) as _ }
    }

    /// Get the number of frames that the buffer has.
    pub fn frames(&self) -> usize {
        unsafe { max_sys::buffer_getframecount(self.buffer) as _ }
    }

    /// Get the sample rate, samples per second, of the buffer data.
    pub fn sample_rate(&self) -> f64 {
        unsafe { max_sys::buffer_getsamplerate(self.buffer) }
    }

    /// Get the sample rate, samples per millisecond, of the buffer data.
    pub fn millisample_rate(&self) -> f64 {
        unsafe { max_sys::buffer_getmillisamplerate(self.buffer) }
    }

    /// Get a slice of samples representing a frame of the given channel.
    pub fn channel_slice(&self, channel: usize) -> Option<&[f32]> {
        if self.channels() > channel {
            let frames = self.frames();
            unsafe {
                Some(std::slice::from_raw_parts(
                    self.samples.offset((channel * frames) as _),
                    frames,
                ))
            }
        } else {
            None
        }
    }

    /// Get a mutable slice of samples representing a frame of the given channel.
    ///
    /// # Remarks
    /// * This method automatically marks the buffer as dirty when this lock is dropped.
    pub fn channel_slice_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        if self.channels() > channel {
            let frames = self.frames();
            self.dirty = true;
            unsafe {
                Some(std::slice::from_raw_parts_mut(
                    self.samples.offset((channel * frames) as _),
                    frames,
                ))
            }
        } else {
            None
        }
    }

    /// Get an iterator to the sample frames.
    /// Each item in the iterator represents a channel of data, starting from the first and ending
    /// with the last.
    pub fn channel_iter(&self) -> impl Iterator<Item = &[f32]> {
        let frames = self.frames();
        let channels = self.channels();
        BufferChannelIter {
            offset: 0,
            samples: self.samples,
            frames,
            channels,
            end: channels * frames,
            _phantom: PhantomData,
        }
    }

    /// Get a mutable iterator to the sample frames.
    /// Each item in the iterator represents a channel of data, starting from the first and ending
    /// with the last.
    ///
    /// # Remarks
    /// * This method automatically marks the buffer as dirty when this lock is dropped.
    pub fn channel_iter_mut(&mut self) -> impl Iterator<Item = &mut [f32]> {
        let frames = self.frames();
        let channels = self.channels();
        self.dirty = true;
        BufferChannelIterMut {
            offset: 0,
            samples: self.samples,
            frames,
            channels,
            end: channels * frames,
            _phantom: PhantomData,
        }
    }

    /// Set this buffer to be marked as dirty when this lock is dropped.
    ///
    /// # Remarks
    /// * You shouldn't have to use this method unless you use the `samples()` method for direct,
    /// `unsafe` data access.
    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn samples(&mut self) -> *mut f32 {
        self.samples
    }
}

impl Index<usize> for BufferLocked {
    type Output = [f32];
    fn index(&self, channel: usize) -> &Self::Output {
        self.channel_slice(channel).expect("channel out of range")
    }
}

impl IndexMut<usize> for BufferLocked {
    fn index_mut(&mut self, channel: usize) -> &mut Self::Output {
        self.channel_slice_mut(channel)
            .expect("channel out of range")
    }
}

impl<'a> Iterator for BufferChannelIter<'a> {
    type Item = &'a [f32];

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset < self.end {
            let offset = self.offset;
            self.offset += self.frames;
            Some(unsafe {
                std::slice::from_raw_parts(self.samples.offset(offset as _), self.frames)
            })
        } else {
            None
        }
    }
}

impl<'a> DoubleEndedIterator for BufferChannelIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.offset < self.end {
            self.end -= self.frames;
            Some(unsafe {
                std::slice::from_raw_parts(self.samples.offset(self.end as _), self.frames)
            })
        } else {
            None
        }
    }
}

impl<'a> ExactSizeIterator for BufferChannelIter<'a> {
    fn len(&self) -> usize {
        self.channels
    }
}

impl<'a> Iterator for BufferChannelIterMut<'a> {
    type Item = &'a mut [f32];

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset < self.end {
            let offset = self.offset;
            self.offset += self.frames;
            Some(unsafe {
                std::slice::from_raw_parts_mut(self.samples.offset(offset as _), self.frames)
            })
        } else {
            None
        }
    }
}

impl<'a> DoubleEndedIterator for BufferChannelIterMut<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.offset < self.end {
            self.end -= self.frames;
            Some(unsafe {
                std::slice::from_raw_parts_mut(self.samples.offset(self.end as _), self.frames)
            })
        } else {
            None
        }
    }
}

impl<'a> ExactSizeIterator for BufferChannelIterMut<'a> {
    fn len(&self) -> usize {
        self.channels
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        unsafe {
            max_sys::object_free(self.value as _);
        }
    }
}

impl Drop for BufferLocked {
    fn drop(&mut self) {
        unsafe {
            if self.dirty {
                max_sys::buffer_setdirty(self.buffer);
            }
            max_sys::buffer_unlocksamples(self.buffer as _);
        }
    }
}
