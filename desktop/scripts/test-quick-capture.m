// Standalone AppKit fixture. Route generalPasteboard to a unique board before opening any UI.
#import "../src-tauri/src/flash.m"
#import <objc/runtime.h>
#include <assert.h>

@interface FixtureVoice : KyDictation
@property(nonatomic, weak) NSTextView *target;
@property(nonatomic) BOOL pending;
@property(nonatomic, copy) void (^notify)(NSString *, BOOL);
@end
@implementation FixtureVoice
- (BOOL)active { return self.target != nil; }
- (BOOL)awaitingAuthorization { return self.pending; }
- (void)startInEditor:(NSTextView *)editor english:(BOOL)english status:(void (^)(NSString *, BOOL))status {
    self.target = editor;
    self.notify = status;
    status(@"private fixture partial", YES);
}
- (void)finishListening { [self stop]; }
- (void)stop {
    if (!self.active) return;
    [self.target insertText:@" transcript" replacementRange:self.target.selectedRange];
    self.target = nil;
    self.notify(@"Inserted", NO);
    self.notify = nil;
}
@end

static bool acceptSave;
static NSUInteger saves, edits;
static void activity(void) { edits++; }
static NSString *savedText, *savedImage;
static bool fixtureSave(const char *text, const char *image) {
    savedText = @(text);
    savedImage = @(image);
    saves++;
    return acceptSave;
}

int main(void) {
    @autoreleasepool {
        NSPasteboard *board = [NSPasteboard pasteboardWithUniqueName];
        method_setImplementation(class_getClassMethod(NSPasteboard.class, @selector(generalPasteboard)),
                                 imp_implementationWithBlock(^NSPasteboard *(id cls){ return board; }));
        [NSApplication sharedApplication];
        // A real status item can be hidden by a full menu bar in CI. Supply a window-backed icon.
        NSRect screen = NSScreen.mainScreen.frame;
        NSWindow *bar = [[NSWindow alloc] initWithContentRect:NSMakeRect(NSMidX(screen), NSMaxY(screen)-24, 30, 24)
            styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        NSButton *anchor = [[NSButton alloc] initWithFrame:bar.contentView.bounds];
        [bar.contentView addSubview:anchor];
        provider = [KyFlash new];
        provider.save = fixtureSave;
        provider.activity = activity;
        [provider openAt:anchor english:YES locked:NO];
        KyFlashEditor *editor = (KyFlashEditor *)provider.body;
        assert(provider.panel.isVisible && provider.panel.firstResponder == editor);
        assert(!provider.panel.canBecomeMainWindow);
        NSRect icon = [anchor.window convertRectToScreen:[anchor convertRect:anchor.bounds toView:nil]];
        assert(NSMaxY(provider.panel.frame) <= NSMinY(icon));
        assert(NSMinX(provider.panel.frame) <= NSMidX(icon) && NSMidX(icon) <= NSMaxX(provider.panel.frame));
        FixtureVoice *voice = [FixtureVoice new];
        provider.dictation = voice;
        [board setString:@"fixture" forType:NSPasteboardTypeString];
        [provider.pasteButton performClick:nil];
        [board clearContents];
        [board setData:[@"fixture bytes" dataUsingEncoding:NSUTF8StringEncoding] forType:NSPasteboardTypePNG];
        NSEvent *paste = [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSZeroPoint modifierFlags:NSEventModifierFlagCommand
            timestamp:0 windowNumber:provider.panel.windowNumber context:nil characters:@"v" charactersIgnoringModifiers:@"v"
            isARepeat:NO keyCode:9];
        assert([editor performKeyEquivalent:paste] && !provider.attachment.hidden);
        [provider dismiss];
        [provider openAt:anchor english:YES locked:NO];
        assert([editor.string isEqualToString:@"fixture"] && provider.image.length);
        [provider.voice performClick:nil];
        assert(voice.active && [provider.voice.title isEqualToString:@"Finish"]);
        assert(edits > 0 && [provider.status.stringValue containsString:@"private fixture"]);
        assert(!provider.status.toolTip.length);
        voice.pending = YES;
        [provider windowDidResignKey:[NSNotification notificationWithName:NSWindowDidResignKeyNotification object:provider.panel]];
        assert(provider.panel.isVisible && voice.active);
        voice.pending = NO;
        [provider.voice performClick:nil];
        assert(!voice.active && [editor.string hasSuffix:@" transcript"]);
        [provider submit:nil];
        assert(saves == 1 && [editor.string isEqualToString:savedText] && savedImage.length);
        assert(provider.panel.isVisible && [provider.status.stringValue containsString:@"retry"]);
        acceptSave = true;
        [provider.voice performClick:nil];
        [provider submit:nil];
        assert(saves == 2 && [savedText isEqualToString:@"fixture transcript transcript"]);
        assert(!provider.panel.isVisible && !provider.image && !editor.string.length);
        [provider openAt:anchor english:NO locked:NO];
        [provider.pasteButton performClick:nil];
        [provider.voice performClick:nil];
        [provider windowDidResignKey:[NSNotification notificationWithName:NSWindowDidResignKeyNotification object:provider.panel]];
        assert(!voice.active && !provider.panel.isVisible && provider.image.length);
        assert(!provider.status.toolTip.length && ![provider.status.stringValue containsString:@"private fixture"]);
        ky_flash_lock();
        [provider openAt:anchor english:YES locked:YES];
        assert(!editor.isEditable && !provider.pasteButton.enabled && !provider.voice.enabled && !provider.commit.enabled);
        assert(!editor.string.length && !provider.image);
        [board releaseGlobally];
        puts("PASS native panel anchor, focus, paste button and shortcut, draft retention, dictation, authorization, save retry, lock");
    }
}
