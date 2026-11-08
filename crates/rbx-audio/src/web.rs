use kira::backend::{Backend, Renderer};
use send_wrapper::SendWrapper;
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{AudioContext, AudioProcessingEvent, ScriptProcessorNode};

const BUFFER_FRAMES: u32 = 1024;

thread_local! {
    static CONTEXT: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
}

pub fn resume() {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref()
            && ctx.state() == web_sys::AudioContextState::Suspended
        {
            let _ = ctx.resume();
        }
    });
}

struct Output {
    ctx: AudioContext,
    node: Option<ScriptProcessorNode>,
    callback: Option<Closure<dyn FnMut(AudioProcessingEvent)>>,
}

pub struct WebBackend {
    output: SendWrapper<Output>,
}

impl Backend for WebBackend {
    type Settings = ();
    type Error = String;

    fn setup(_: (), _internal_buffer_size: usize) -> Result<(Self, u32), String> {
        let ctx = AudioContext::new().map_err(|e| format!("{e:?}"))?;
        let rate = ctx.sample_rate() as u32;
        CONTEXT.with(|c| *c.borrow_mut() = Some(ctx.clone()));
        Ok((
            WebBackend {
                output: SendWrapper::new(Output {
                    ctx,
                    node: None,
                    callback: None,
                }),
            },
            rate,
        ))
    }

    fn start(&mut self, mut renderer: Renderer) -> Result<(), String> {
        let out = &mut *self.output;
        let node = out
            .ctx
            .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                BUFFER_FRAMES,
                0,
                2,
            )
            .map_err(|e| format!("{e:?}"))?;
        let mut interleaved = vec![0.0f32; BUFFER_FRAMES as usize * 2];
        let mut left = vec![0.0f32; BUFFER_FRAMES as usize];
        let mut right = vec![0.0f32; BUFFER_FRAMES as usize];
        let callback =
            Closure::wrap_assert_unwind_safe(Box::new(move |event: AudioProcessingEvent| {
                let Ok(buffer) = event.output_buffer() else {
                    return;
                };
                let frames = buffer.length() as usize;
                interleaved.resize(frames * 2, 0.0);
                left.resize(frames, 0.0);
                right.resize(frames, 0.0);
                interleaved.fill(0.0);
                renderer.on_start_processing();
                renderer.process(&mut interleaved, 2);
                for i in 0..frames {
                    left[i] = interleaved[i * 2];
                    right[i] = interleaved[i * 2 + 1];
                }
                let _ = buffer.copy_to_channel(&left, 0);
                let _ = buffer.copy_to_channel(&right, 1);
            })
                as Box<dyn FnMut(AudioProcessingEvent)>);
        node.set_onaudioprocess(Some(callback.as_ref().unchecked_ref()));
        node.connect_with_audio_node(&out.ctx.destination())
            .map_err(|e| format!("{e:?}"))?;
        out.node = Some(node);
        out.callback = Some(callback);
        Ok(())
    }
}
