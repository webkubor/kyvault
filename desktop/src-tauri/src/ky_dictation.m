#import "ky_dictation.h"
#import <AVFoundation/AVFoundation.h>
#import <Speech/Speech.h>

typedef void (^KyVoiceStatus)(NSString *, BOOL);

@interface KyDictation ()
@property(nonatomic, readwrite) BOOL active;
@property(nonatomic, readwrite) BOOL awaitingAuthorization;
@property(nonatomic) NSUInteger generation;
@property(nonatomic) BOOL tapped;
@property(nonatomic) BOOL english;
@property(nonatomic, weak) NSTextView *editor;
@property(nonatomic, copy) KyVoiceStatus status;
@property(nonatomic, copy) NSString *transcript;
@property(nonatomic, strong) AVAudioEngine *engine;
@property(nonatomic, strong) SFSpeechRecognizer *recognizer;
@property(nonatomic, strong) SFSpeechAudioBufferRecognitionRequest *request;
@property(nonatomic, strong) SFSpeechRecognitionTask *task;
@end

@implementation KyDictation
- (void)finish:(NSString *)message {
    if (!self.active) return;
    self.active = NO;
    self.awaitingAuthorization = NO;
    self.generation++;
    [self.engine stop];
    if (self.tapped) [self.engine.inputNode removeTapOnBus:0];
    self.tapped = NO;
    [self.request endAudio];
    [self.task cancel];
    if (self.transcript.length && self.editor)
        [self.editor insertText:self.transcript replacementRange:self.editor.selectedRange];
    KyVoiceStatus notify = self.status;
    self.task = nil;
    self.request = nil;
    self.engine = nil;
    self.recognizer = nil;
    self.editor = nil;
    self.transcript = nil;
    self.status = nil;
    if (notify) notify(message, NO);
}
- (void)stop {
    [self finish:self.transcript.length
        ? (self.english ? @"Text inserted" : @"已插入听写文字")
        : (self.english ? @"Dictation stopped" : @"听写已停止")];
}
- (void)finishListening {
    if (!self.active || !self.tapped) { [self stop]; return; }
    [self.engine stop];
    [self.engine.inputNode removeTapOnBus:0];
    self.tapped = NO;
    [self.request endAudio];
    self.status(self.english ? @"Finishing dictation…" : @"正在整理最后一句…", YES);
    NSUInteger generation = self.generation;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        if (self.active && self.generation == generation) [self stop];
    });
}
- (void)listen:(NSUInteger)generation {
    self.engine = [AVAudioEngine new];
    AVAudioInputNode *input = self.engine.inputNode;
    AVAudioFormat *format = [input outputFormatForBus:0];
    if (!format.channelCount || format.sampleRate <= 0) {
        [self finish:self.english ? @"Connect a microphone and try again." : @"请连接麦克风后重试。"];
        return;
    }
    SFSpeechAudioBufferRecognitionRequest *request = [SFSpeechAudioBufferRecognitionRequest new];
    request.requiresOnDeviceRecognition = YES;
    request.shouldReportPartialResults = YES;
    self.request = request;
    __weak KyDictation *weakSelf = self;
    self.task = [self.recognizer recognitionTaskWithRequest:request resultHandler:
        ^(SFSpeechRecognitionResult *result, NSError *error) {
            dispatch_async(dispatch_get_main_queue(), ^{
                KyDictation *voice = weakSelf;
                if (!voice.active || voice.generation != generation) return;
                if (result.bestTranscription.formattedString.length)
                    voice.transcript = result.bestTranscription.formattedString;
                if (result.isFinal) [voice stop];
                else if (error) [voice finish:voice.english
                    ? @"Dictation ended. Check the microphone and offline dictation language."
                    : @"听写已结束。请检查麦克风和系统听写语言。"];
                else if (voice.transcript.length) voice.status(voice.transcript, YES);
            });
        }];
    [input installTapOnBus:0 bufferSize:1024 format:format
                     block:^(AVAudioPCMBuffer *buffer, AVAudioTime *when) {
        [request appendAudioPCMBuffer:buffer];
    }];
    self.tapped = YES;
    NSError *error = nil;
    if (![self.engine startAndReturnError:&error]) {
        [self finish:self.english ? @"Microphone unavailable. Try again." : @"麦克风不可用，请重试。"];
        return;
    }
    self.status(self.english ? @"Listening… tap to finish" : @"正在听写，再点一次结束", YES);
}
- (void)startInEditor:(NSTextView *)editor english:(BOOL)english status:(KyVoiceStatus)status {
    if (self.active || !editor || !status) return;
    self.active = YES;
    self.english = english;
    self.editor = editor;
    self.status = status;
    NSUInteger generation = ++self.generation;
    NSLocale *locale = [NSLocale localeWithLocaleIdentifier:english ? @"en-US" : @"zh-CN"];
    self.recognizer = [[SFSpeechRecognizer alloc] initWithLocale:locale];
    if (!self.recognizer.supportsOnDeviceRecognition) {
        [self finish:english ? @"Enable an offline dictation language in Keyboard settings."
                             : @"请在系统设置的「键盘 → 听写」中启用本机听写语言。"];
        return;
    }
    self.awaitingAuthorization = YES;
    status(english ? @"Checking microphone and speech permissions…" : @"正在检查麦克风与语音识别权限…", YES);
    [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus auth) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (!self.active || self.generation != generation) return;
            if (auth != SFSpeechRecognizerAuthorizationStatusAuthorized) {
                [self finish:english ? @"Allow Ky in Privacy & Security → Speech Recognition."
                                     : @"请在「隐私与安全性 → 语音识别」中允许 Ky。"];
                return;
            }
            [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
                dispatch_async(dispatch_get_main_queue(), ^{
                    if (!self.active || self.generation != generation) return;
                    if (!granted) {
                        [self finish:english ? @"Allow Ky in Privacy & Security → Microphone."
                                             : @"请在「隐私与安全性 → 麦克风」中允许 Ky。"];
                        return;
                    }
                    self.awaitingAuthorization = NO;
                    if (!self.editor.window.isKeyWindow) {
                        [self finish:english ? @"Microphone ready. Click Dictate to start."
                                             : @"麦克风已就绪，点「听写」开始。"];
                        return;
                    }
                    [self listen:generation];
                });
            }];
        });
    }];
}
@end
