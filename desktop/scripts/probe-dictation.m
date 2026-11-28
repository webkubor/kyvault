// Optional, file-only probe. No authorization requests or audio input nodes.
#import <Foundation/Foundation.h>
#import <Speech/Speech.h>

int main(int argc, char **argv) { @autoreleasepool {
    if (argc != 2) return 2;
    NSURL *url = [NSURL fileURLWithPath:@(argv[1])];
    SFSpeechRecognizer *recognizer = [[SFSpeechRecognizer alloc]
        initWithLocale:[NSLocale localeWithLocaleIdentifier:@"en-US"]];
    SFSpeechRecognizerAuthorizationStatus authorization = SFSpeechRecognizer.authorizationStatus;
    printf("PROBE en-US on_device=%d available=%d speech_authorization=%ld\n",
           recognizer.supportsOnDeviceRecognition, recognizer.available, (long)authorization);
    if (!recognizer.supportsOnDeviceRecognition || !recognizer.available ||
        authorization != SFSpeechRecognizerAuthorizationStatusAuthorized) {
        puts("SKIP offline transcription: existing authorization or model unavailable");
        return 77;
    }
    SFSpeechURLRecognitionRequest *request = [[SFSpeechURLRecognitionRequest alloc] initWithURL:url];
    request.requiresOnDeviceRecognition = YES;
    request.shouldReportPartialResults = NO;
    recognizer.queue = NSOperationQueue.mainQueue;
    __block BOOL done = NO, matched = NO;
    SFSpeechRecognitionTask *task = [recognizer recognitionTaskWithRequest:request
        resultHandler:^(SFSpeechRecognitionResult *result, NSError *error) {
            if (!result.isFinal && !error) return;
            if (error) NSLog(@"PROBE error %@ %ld", error.domain, (long)error.code);
            NSString *text = result.bestTranscription.formattedString.lowercaseString;
            matched = !error && text && [text containsString:@"purple garden"] &&
                      [text containsString:@"flowers"];
            done = YES;
        }];
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:20];
    while (!done && deadline.timeIntervalSinceNow > 0)
        [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    [task cancel];
    puts(matched ? "PASS synthetic file recognized offline" : "FAIL synthetic file not recognized");
    return matched ? 0 : 1;
} }
