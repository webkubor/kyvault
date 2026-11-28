#import <AppKit/AppKit.h>

/// One user-initiated, on-device dictation session. Call on the main thread.
@interface KyDictation : NSObject
@property(nonatomic, readonly) BOOL active;
@property(nonatomic, readonly) BOOL awaitingAuthorization;
- (void)startInEditor:(NSTextView *)editor english:(BOOL)english
               status:(void (^)(NSString *message, BOOL active))status;
// Finish the utterance, allowing the recognizer to deliver its final words.
- (void)finishListening;
// Synchronously release the microphone and insert the latest recognized text.
- (void)stop;
@end
