// Compile the production state machine with hardware and permission APIs replaced.
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <Speech/Speech.h>
#include <stdbool.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"FAIL %d: %s\n",__LINE__,#x); exit(1); } } while (0)

typedef void (^SpeechReply)(SFSpeechRecognizerAuthorizationStatus);
typedef void (^MicReply)(BOOL);
typedef void (^RecognitionReply)(SFSpeechRecognitionResult *, NSError *);
static SpeechReply speech;
static MicReply mic;
static RecognitionReply result;
static int starts, taps, removals, ends, cancels;
static BOOL running;

static NSMutableArray<NSString *> *openedSettings;
static AVAuthorizationStatus micStatus = AVAuthorizationStatusDenied;
@interface FixtureSettings : NSObject
+ (instancetype)sharedWorkspace;
@end
@implementation FixtureSettings
+ (instancetype)sharedWorkspace { return [self new]; }
- (BOOL)openURL:(NSURL *)url { [openedSettings addObject:url.absoluteString]; return YES; }
@end

@interface FakeDevice : NSObject
@end
@implementation FakeDevice
+ (AVAuthorizationStatus)authorizationStatusForMediaType:(AVMediaType)media { return micStatus; }
+ (void)requestAccessForMediaType:(AVMediaType)media completionHandler:(MicReply)reply {
    CHECK([media isEqual:AVMediaTypeAudio]); mic = [reply copy];
}
@end

@interface FakeInput : NSObject
@end
@implementation FakeInput
- (AVAudioFormat *)outputFormatForBus:(AVAudioNodeBus)bus {
    return [[AVAudioFormat alloc] initStandardFormatWithSampleRate:16000 channels:1];
}
- (void)installTapOnBus:(AVAudioNodeBus)bus bufferSize:(AVAudioFrameCount)size
                 format:(AVAudioFormat *)format block:(AVAudioNodeTapBlock)block { taps++; }
- (void)removeTapOnBus:(AVAudioNodeBus)bus { removals++; }
@end
@interface FakeEngine : NSObject
@property(strong) FakeInput *inputNode;
@end
@implementation FakeEngine
- (instancetype)init { if ((self = [super init])) _inputNode = [FakeInput new]; return self; }
- (BOOL)startAndReturnError:(NSError **)error { running = YES; starts++; return YES; }
- (void)stop { running = NO; }
@end
@interface FakeRequest : NSObject
@property BOOL requiresOnDeviceRecognition, shouldReportPartialResults;
@end
@implementation FakeRequest
- (void)appendAudioPCMBuffer:(AVAudioPCMBuffer *)buffer {}
- (void)endAudio { ends++; }
@end
@interface FakeTask : NSObject
@end
@implementation FakeTask
- (void)cancel { cancels++; }
@end
@interface FakeRecognizer : NSObject
@end
@implementation FakeRecognizer
- (instancetype)initWithLocale:(NSLocale *)locale { return [super init]; }
- (BOOL)supportsOnDeviceRecognition { return YES; }
+ (void)requestAuthorization:(SpeechReply)reply { speech = [reply copy]; }
- (FakeTask *)recognitionTaskWithRequest:(FakeRequest *)request resultHandler:(RecognitionReply)reply {
    CHECK(request.requiresOnDeviceRecognition && request.shouldReportPartialResults);
    result = [reply copy]; return [FakeTask new];
}
@end

#define NSWorkspace FixtureSettings
#define AVCaptureDevice FakeDevice
#define AVAudioEngine FakeEngine
#define AVAudioInputNode FakeInput
#define SFSpeechAudioBufferRecognitionRequest FakeRequest
#define SFSpeechRecognitionTask FakeTask
#define SFSpeechRecognizer FakeRecognizer
#import "../src-tauri/src/ky_dictation.m"
#undef NSWorkspace

@interface FakeResult : NSObject
@property(copy) NSString *formattedString;
@property BOOL isFinal;
@end
@implementation FakeResult
- (id)bestTranscription { return self; }
@end

static void drain(void) {
    __block BOOL done = NO;
    dispatch_async(dispatch_get_main_queue(), ^{ done = YES; });
    while (!done) [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.005]];
}
static void emit(RecognitionReply reply, NSString *text, BOOL final) {
    FakeResult *r = [FakeResult new];
    r.formattedString = text; r.isFinal = final;
    reply((id)r, nil); drain();
}

// Real flash submit, in-memory save callback instead of the vault.
#import "../src-tauri/src/flash.m"
static NSString *saved;
static bool saveTextFixture(const char *text) { saved = @(text); return true; }
static bool saveAttachmentFixture(const char *text, const char *image) {
    CHECK(!image || !*image);
    return saveTextFixture(text);
}

// Model a focused editor without presenting a panel or activating Ky.
@interface FixtureWindow : NSWindow
@end
@implementation FixtureWindow
- (BOOL)isKeyWindow { return YES; }
@end

int main(void) { @autoreleasepool {
    [NSApplication sharedApplication];
    NSTextView *editor = [[NSTextView alloc] initWithFrame:NSMakeRect(0,0,300,100)];
    FixtureWindow *window = [[FixtureWindow alloc] initWithContentRect:editor.frame
        styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:YES];
    window.contentView = editor;
    KyDictation *voice = [KyDictation new];
    __block NSString *message;
    __block int notifications = 0;
    void (^begin)(void) = ^{
        speech = nil; mic = nil;
        [voice startInEditor:editor english:YES status:^(NSString *s, BOOL active) {
            CHECK(active == voice.active); message = s; notifications++;
        }];
        CHECK(voice.active && speech);
    };
    void (^grantSpeech)(void) = ^{
        speech(SFSpeechRecognizerAuthorizationStatusAuthorized); drain(); CHECK(mic);
    };

    openedSettings = [NSMutableArray new];
    editor.string = @"draft";
    begin(); speech(SFSpeechRecognizerAuthorizationStatusDenied); drain();
    CHECK(!voice.active && !mic && !starts && [message containsString:@"Speech Recognition"]);
    begin(); grantSpeech(); mic(NO); drain();
    CHECK(!voice.active && !starts && [message containsString:@"Microphone"]);
    CHECK(([openedSettings isEqual:@[@"x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition",
                                    @"x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"]]));
    begin(); speech(SFSpeechRecognizerAuthorizationStatusRestricted); drain();
    micStatus = AVAuthorizationStatusRestricted;
    begin(); grantSpeech(); mic(NO); drain();
    CHECK(openedSettings.count == 2 && !starts && [editor.string isEqual:@"draft"]);
    micStatus = AVAuthorizationStatusDenied;
    begin(); [voice stop]; speech(SFSpeechRecognizerAuthorizationStatusDenied); drain();
    CHECK(openedSettings.count == 2);
    puts("PASS direct privacy links, preserved draft, restricted and stale requests");

    begin(); SpeechReply oldSpeech = speech;
    [voice stop]; begin(); oldSpeech(SFSpeechRecognizerAuthorizationStatusAuthorized); drain();
    CHECK(voice.active && !mic);
    grantSpeech(); MicReply oldMic = mic;
    [voice stop]; begin(); oldMic(YES); drain();
    CHECK(voice.active && !starts);
    grantSpeech(); mic(YES); drain();
    CHECK(running && starts == 1 && taps == 1);
    puts("PASS stale permission callbacks after restart");

    editor.string = @"prefix [x] suffix";
    editor.selectedRange = [editor.string rangeOfString:@"[x]"];
    emit(result, @"one", NO); emit(result, @"one two", NO);
    CHECK([message isEqual:@"one two"] && [editor.string containsString:@"[x]"]);
    RecognitionReply oldResult = result;
    KyFlash *flash = [KyFlash new];
    flash.body = editor;
    flash.save = _Generic((KySave)0, bool (*)(const char *): saveTextFixture,
                          bool (*)(const char *, const char *): saveAttachmentFixture);
    if ([flash respondsToSelector:@selector(setDictation:)]) {
        [flash setValue:voice forKey:@"dictation"];
    } else {
        // At 4f1ee81 flash has no dictation controller. Exercise its two public steps.
        [voice stop];
        CHECK([editor.string isEqual:@"prefix one two suffix"]);
    }
    [flash submit:nil]; [voice stop];
    CHECK(!voice.active && !running && removals == 1 && ends == 1 && cancels == 1);
    CHECK([saved isEqual:@"prefix one two suffix"] && !editor.string.length);
    puts("PASS latest partial inserted once at selection; flash submit saves and clears");

    begin(); grantSpeech(); mic(YES); drain();
    int before = notifications;
    emit(oldResult, @"stale final", YES);
    CHECK(voice.active && notifications == before && !editor.string.length);
    [voice finishListening];
    CHECK(voice.active && !running && removals == 2 && ends == 2);
    emit(result, @"final words", YES);
    CHECK(!voice.active && [editor.string isEqual:@"final words"] && cancels == 2);

    begin(); grantSpeech(); mic(YES); drain();
    [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:2.2]];
    CHECK(voice.active && running); // Timer from the previous session has expired.
    emit(result, @" timeout", NO);
    [voice finishListening];
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:4];
    while (voice.active && deadline.timeIntervalSinceNow > 0)
        [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
    CHECK(!voice.active && !running && removals == 3 && cancels == 3);
    CHECK([editor.string isEqual:@"final words timeout"]);
    puts("PASS stale result and timer; stop input, await final or flush on timeout");
    return 0;
} }
