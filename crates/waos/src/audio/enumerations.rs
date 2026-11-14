//! The spec's enumerations, between the WIT's and web-audio-api's, which name the same values.

use waos_capabilities::web_audio_api::AutomationRate as CrateAutomationRate;
use waos_capabilities::web_audio_api::context::{
    AudioContextLatencyCategory as CrateLatencyCategory, AudioContextState as CrateState,
};
use waos_capabilities::web_audio_api::node::{
    BiquadFilterType as CrateBiquadFilterType, ChannelCountMode as CrateChannelCountMode,
    ChannelInterpretation as CrateChannelInterpretation,
    DistanceModelType as CrateDistanceModelType, OscillatorType as CrateOscillatorType,
    OverSampleType as CrateOverSampleType, PanningModelType as CratePanningModelType,
};

use crate::bindings::wasdk::platform::audio::{
    AudioContextLatencyCategory, AudioContextState, AutomationRate, BiquadFilterType,
    ChannelCountMode, ChannelInterpretation, DistanceModelType, OscillatorType, OverSampleType,
    PanningModelType,
};

/// Converts both ways between a WIT enumeration and web-audio-api's, case for case.
macro_rules! same_cases {
    ($wit:ident, $crate_type:ident, [$($wit_case:ident = $crate_case:ident),+ $(,)?]) => {
        impl From<$crate_type> for $wit {
            fn from(value: $crate_type) -> $wit {
                match value {
                    $($crate_type::$crate_case => $wit::$wit_case,)+
                }
            }
        }

        impl From<$wit> for $crate_type {
            fn from(value: $wit) -> $crate_type {
                match value {
                    $($wit::$wit_case => $crate_type::$crate_case,)+
                }
            }
        }
    };
}

same_cases!(
    AudioContextState,
    CrateState,
    [Suspended = Suspended, Running = Running, Closed = Closed,]
);
same_cases!(
    ChannelCountMode,
    CrateChannelCountMode,
    [Max = Max, ClampedMax = ClampedMax, Explicit = Explicit,]
);
same_cases!(
    ChannelInterpretation,
    CrateChannelInterpretation,
    [Speakers = Speakers, Discrete = Discrete,]
);
same_cases!(AutomationRate, CrateAutomationRate, [ARate = A, KRate = K]);
same_cases!(
    OscillatorType,
    CrateOscillatorType,
    [
        Sine = Sine,
        Square = Square,
        Sawtooth = Sawtooth,
        Triangle = Triangle,
        Custom = Custom,
    ]
);
same_cases!(
    BiquadFilterType,
    CrateBiquadFilterType,
    [
        Lowpass = Lowpass,
        Highpass = Highpass,
        Bandpass = Bandpass,
        Lowshelf = Lowshelf,
        Highshelf = Highshelf,
        Peaking = Peaking,
        Notch = Notch,
        Allpass = Allpass,
    ]
);
same_cases!(
    OverSampleType,
    CrateOverSampleType,
    [None = None, X2 = X2, X4 = X4]
);
same_cases!(
    PanningModelType,
    CratePanningModelType,
    [Equalpower = EqualPower, Hrtf = HRTF,]
);
same_cases!(
    DistanceModelType,
    CrateDistanceModelType,
    [
        Linear = Linear,
        Inverse = Inverse,
        Exponential = Exponential,
    ]
);

impl From<AudioContextLatencyCategory> for CrateLatencyCategory {
    fn from(category: AudioContextLatencyCategory) -> CrateLatencyCategory {
        match category {
            AudioContextLatencyCategory::Balanced => CrateLatencyCategory::Balanced,
            AudioContextLatencyCategory::Interactive => CrateLatencyCategory::Interactive,
            AudioContextLatencyCategory::Playback => CrateLatencyCategory::Playback,
        }
    }
}
