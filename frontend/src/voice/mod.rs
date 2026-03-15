//! Voice（语音输入输出）
//!
//! 负责桌宠侧的音频采集与播放，以及与后端 Voice Service 的协作：
//! - mic_capture：麦克风采集与音频帧处理
//! - speech_recognition：对接 STT（本地或调用后端）
//! - audio_playback：对接 TTS 音频播放（本地或拉取后端）

