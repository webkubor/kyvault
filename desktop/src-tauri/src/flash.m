#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <dispatch/dispatch.h>
#include <stdbool.h>

typedef bool (*KySave)(const char *);

@interface KyFlashPanel : NSPanel
@end

@implementation KyFlashPanel
- (BOOL)canBecomeKeyWindow { return YES; }
@end

@interface KyFlash : NSObject
@property(nonatomic) KySave save;
@property(nonatomic, strong) KyFlashPanel *panel;
@property(nonatomic, strong) NSTextView *body;
@property(nonatomic, strong) NSTextField *status;
@property(nonatomic, strong) NSRunningApplication *previousApp;
@end

@interface KyFlashEditor : NSTextView
@property(nonatomic, weak) KyFlash *owner;
@end

@interface KyFlash (Actions)
- (void)submit:(id)sender;
- (void)dismiss;
@end

@implementation KyFlashEditor
- (BOOL)performKeyEquivalent:(NSEvent *)event {
    NSEventModifierFlags mods = event.modifierFlags &
        (NSEventModifierFlagCommand | NSEventModifierFlagShift |
         NSEventModifierFlagOption | NSEventModifierFlagControl);
    NSString *key = event.charactersIgnoringModifiers;
    if (!self.hasMarkedText && mods == NSEventModifierFlagCommand &&
        ([key isEqualToString:@"\r"] || [key isEqualToString:@"\x03"])) {
        [self.owner submit:self];
        return YES;
    }
    return [super performKeyEquivalent:event];
}
- (void)cancelOperation:(id)sender { [self.owner dismiss]; }
@end

// Synchronous so a returning lock has already discarded the draft.
static void onMain(dispatch_block_t action) {
    if (NSThread.isMainThread) action();
    else dispatch_sync(dispatch_get_main_queue(), action);
}

@implementation KyFlash
- (void)buildPanel {
    NSRect frame = NSMakeRect(0, 0, 460, 240);
    self.panel = [[KyFlashPanel alloc] initWithContentRect:frame
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskResizable |
                  NSWindowStyleMaskUtilityWindow
        backing:NSBackingStoreBuffered defer:NO];
    self.panel.title = @"Ky 闪记";
    self.panel.contentMinSize = NSMakeSize(300, 160);
    self.panel.floatingPanel = YES;
    self.panel.hidesOnDeactivate = YES;
    self.panel.releasedWhenClosed = NO;
    self.panel.restorable = NO;

    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(12, 48, 436, 180)];
    scroll.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    scroll.hasVerticalScroller = YES;
    KyFlashEditor *editor = [[KyFlashEditor alloc] initWithFrame:(NSRect){NSZeroPoint, scroll.contentSize}];
    editor.owner = self;
    editor.font = [NSFont systemFontOfSize:15];
    editor.richText = NO;
    editor.allowsUndo = YES;
    editor.automaticTextReplacementEnabled = NO;
    editor.automaticSpellingCorrectionEnabled = NO;
    self.panel.sharingType = NSWindowSharingNone;
    editor.verticallyResizable = YES;
    editor.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
    editor.autoresizingMask = NSViewWidthSizable;
    editor.textContainer.widthTracksTextView = YES;
    editor.textContainerInset = NSMakeSize(6, 6);
    self.body = editor;
    scroll.documentView = editor;
    [self.panel.contentView addSubview:scroll];
    self.panel.initialFirstResponder = editor;

    NSButton *button = [NSButton buttonWithTitle:@"保存" target:self action:@selector(submit:)];
    button.frame = NSMakeRect(370, 8, 78, 32);
    button.autoresizingMask = NSViewMinXMargin;
    [self.panel.contentView addSubview:button];
    self.status = [NSTextField labelWithString:@"⌘↩ 保存到私密收藏"];
    self.status.frame = NSMakeRect(16, 15, 340, 19);
    self.status.autoresizingMask = NSViewWidthSizable;
    [self.panel.contentView addSubview:self.status];
    [self.panel center];
}

- (void)open {
    if (!self.panel) [self buildPanel];
    NSRunningApplication *front = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (front && ![front isEqual:NSRunningApplication.currentApplication])
        self.previousApp = front;
    else if (!self.panel.isVisible)
        self.previousApp = nil;
    if (@available(macOS 14.0, *)) [NSApp activate];
    else [NSApp activateIgnoringOtherApps:YES];
    [self.panel makeKeyAndOrderFront:nil];
    [self.panel makeFirstResponder:self.body];
}

- (void)clearDraft {
    [self.body.inputContext discardMarkedText];
    self.body.string = @"";
    [self.body.undoManager removeAllActions];
    self.status.stringValue = @"⌘↩ 保存到私密收藏";
}
- (void)dismiss {
    [self.panel orderOut:nil];
    NSRunningApplication *previous = self.previousApp;
    self.previousApp = nil;
    if (previous && !previous.terminated) {
        if (@available(macOS 14.0, *)) [NSApp yieldActivationToApplication:previous];
        [previous activateWithOptions:0];
    }
}

- (NSString *)saveText:(NSString *)text {
    if (![text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length)
        return @"没有可保存的正文。";
    if ([text containsString:[NSString stringWithCharacters:(unichar[]){0} length:1]])
        return @"正文含有无法保存的空字符。";
    const char *utf8 = text.UTF8String;
    if (!self.save || !utf8 || !self.save(utf8))
        return @"保存失败，请重试。";
    return nil;
}
- (void)submit:(id)sender {
    NSString *error = [self saveText:self.body.string];
    if (error) {
        self.status.stringValue = error;
        [self.panel makeFirstResponder:self.body];
        return;
    }
    [self clearDraft];
    [self dismiss];
}

- (void)saveSelection:(NSPasteboard *)pasteboard userData:(NSString *)data
                error:(NSString * __autoreleasing *)error {
    __block NSString *message;
    onMain(^{
        message = [self saveText:[pasteboard stringForType:NSPasteboardTypeString]];
    });
    if (error) *error = message;
}
@end

static KyFlash *provider;

void ky_flash_setup(bool (*save)(const char *)) {
    onMain(^{
        if (!provider) provider = [KyFlash new];
        provider.save = save;
        [NSApplication sharedApplication].servicesProvider = provider;
        NSUpdateDynamicServices();
    });
}

void ky_flash_open(void) {
    onMain(^{ [provider open]; });
}

void ky_flash_lock(void) {
    onMain(^{
        [provider.panel orderOut:nil];
        [provider clearDraft];
        provider.previousApp = nil;
    });
}

const char *ky_flash_choose_app(void) {
    static char identifier[256];
    NSOpenPanel *panel = [NSOpenPanel openPanel];
    panel.directoryURL = [NSURL fileURLWithPath:@"/Applications"];
    panel.allowedContentTypes = @[UTTypeApplicationBundle];
    panel.prompt = @"选择应用";
    if ([panel runModal] != NSModalResponseOK) return NULL;
    NSString *bundle = [NSBundle bundleWithURL:panel.URL].bundleIdentifier;
    return [bundle getCString:identifier maxLength:sizeof(identifier) encoding:NSUTF8StringEncoding] ? identifier : NULL;
}
