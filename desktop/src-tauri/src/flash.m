#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import "ky_dictation.h"

typedef bool (*KySave)(const char *text, const char *image);
typedef void (*KyActivity)(void);

@interface KyFlashPanel : NSPanel
@end
@implementation KyFlashPanel
- (BOOL)canBecomeKeyWindow { return YES; }
- (BOOL)canBecomeMainWindow { return NO; }
@end

@interface KyFlash : NSObject <NSWindowDelegate, NSTextViewDelegate>
@property(nonatomic) KySave save;
@property(nonatomic) KyActivity activity;
@property(nonatomic, strong) KyFlashPanel *panel;
@property(nonatomic, strong) NSTextView *body;
@property(nonatomic, strong) NSTextField *heading, *status;
@property(nonatomic, strong) NSButton *voice, *commit, *attachment, *hide, *pasteButton;
@property(nonatomic, strong) KyDictation *dictation;
@property(nonatomic, copy) NSString *image;
@property(nonatomic) BOOL english;
- (void)submit:(id)sender;
- (void)dismiss;
- (void)pasteImage:(NSPasteboard *)board type:(NSPasteboardType)type;
@end

@interface KyFlashEditor : NSTextView
@property(nonatomic, weak) KyFlash *owner;
- (void)pasteFrom:(NSPasteboard *)board;
@end

@implementation KyFlashEditor
- (BOOL)performKeyEquivalent:(NSEvent *)event {
    NSEventModifierFlags mods = event.modifierFlags &
        (NSEventModifierFlagCommand | NSEventModifierFlagShift |
         NSEventModifierFlagOption | NSEventModifierFlagControl);
    NSString *key = event.charactersIgnoringModifiers;
    if (mods == NSEventModifierFlagCommand && [key.lowercaseString isEqualToString:@"v"] && self.isEditable) {
        [self paste:self];
        return YES;
    }
    if (!self.hasMarkedText && mods == NSEventModifierFlagCommand &&
        ([key isEqualToString:@"\r"] || [key isEqualToString:@"\x03"])) {
        [self.owner submit:self];
        return YES;
    }
    return [super performKeyEquivalent:event];
}
- (void)cancelOperation:(id)sender { [self.owner dismiss]; }
- (void)paste:(id)sender { [self pasteFrom:NSPasteboard.generalPasteboard]; }
- (void)pasteFrom:(NSPasteboard *)board {
    if (!self.isEditable) return;
    NSPasteboardType type = [board availableTypeFromArray:@[NSPasteboardTypePNG, UTTypeJPEG.identifier,
                                                            UTTypeWebP.identifier, NSPasteboardTypeTIFF]];
    if (type) { [self.owner pasteImage:board type:type]; return; }
    NSString *text = [board stringForType:NSPasteboardTypeString];
    if (text.length) [self insertText:text replacementRange:self.selectedRange];
}
@end

static void onMain(dispatch_block_t action) {
    if (NSThread.isMainThread) action();
    else dispatch_sync(dispatch_get_main_queue(), action);
}

@implementation KyFlash
- (NSString *)hint {
    return self.english ? @"Type or paste text or an image" : @"输入文字，或粘贴文字、图片";
}
- (NSString *)voiceTitle { return self.english ? @"Dictate" : @"听写"; }
- (NSButton *)button:(SEL)action frame:(NSRect)frame {
    NSButton *button = [NSButton buttonWithTitle:@"" target:self action:action];
    button.bordered = NO;
    button.frame = frame;
    [self.panel.contentView addSubview:button];
    return button;
}
- (void)buildPanel {
    NSRect frame = NSMakeRect(0, 0, 540, 166);
    self.panel = [[KyFlashPanel alloc] initWithContentRect:frame
        styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
        backing:NSBackingStoreBuffered defer:NO];
    self.panel.delegate = self;
    self.panel.floatingPanel = YES;
    self.panel.level = NSStatusWindowLevel;
    self.panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    self.panel.releasedWhenClosed = NO;
    self.panel.restorable = NO;
    self.panel.sharingType = NSWindowSharingNone;
    self.panel.opaque = NO;
    self.panel.backgroundColor = NSColor.clearColor;
    self.panel.hasShadow = YES;
    NSVisualEffectView *surface = [[NSVisualEffectView alloc] initWithFrame:frame];
    surface.material = NSVisualEffectMaterialPopover;
    surface.state = NSVisualEffectStateActive;
    surface.wantsLayer = YES;
    surface.layer.cornerRadius = 14;
    surface.layer.masksToBounds = YES;
    self.panel.contentView = surface;
    self.heading = [NSTextField labelWithString:@""];
    self.heading.font = [NSFont boldSystemFontOfSize:14];
    self.heading.frame = NSMakeRect(16, 131, 210, 22);
    [surface addSubview:self.heading];
    self.attachment = [self button:@selector(removeImage:) frame:NSMakeRect(297,127,140,30)];
    self.attachment.hidden = YES;
    self.hide = [self button:@selector(dismiss) frame:NSMakeRect(441,127,86,30)];
    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(14, 49, 512, 75)];
    scroll.hasVerticalScroller = YES;
    scroll.drawsBackground = NO;
    KyFlashEditor *editor = [[KyFlashEditor alloc] initWithFrame:(NSRect){NSZeroPoint, scroll.contentSize}];
    editor.owner = self;
    editor.delegate = self;
    editor.font = [NSFont systemFontOfSize:16];
    editor.drawsBackground = NO;
    editor.richText = NO;
    editor.allowsUndo = YES;
    editor.automaticTextReplacementEnabled = NO;
    editor.automaticTextCompletionEnabled = NO;
    editor.continuousSpellCheckingEnabled = NO;
    editor.automaticSpellingCorrectionEnabled = NO;
    editor.verticallyResizable = YES;
    editor.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
    editor.autoresizingMask = NSViewWidthSizable;
    editor.textContainer.widthTracksTextView = YES;
    editor.textContainerInset = NSMakeSize(4, 8);
    self.body = editor;
    scroll.documentView = editor;
    [surface addSubview:scroll];
    self.panel.initialFirstResponder = editor;
    self.status = [NSTextField wrappingLabelWithString:@""];
    self.status.font = [NSFont systemFontOfSize:12];
    self.status.textColor = NSColor.secondaryLabelColor;
    self.status.frame = NSMakeRect(16, 5, 222, 38);
    [surface addSubview:self.status];
    self.pasteButton = [self button:@selector(pasteNote:) frame:NSMakeRect(244,10,80,32)];
    self.pasteButton.image = [NSImage imageWithSystemSymbolName:@"doc.on.clipboard" accessibilityDescription:nil];
    self.pasteButton.imagePosition = NSImageLeading;
    self.voice = [self button:@selector(dictate:) frame:NSMakeRect(320,10,100,32)];
    self.commit = [self button:@selector(submit:) frame:NSMakeRect(419,10,110,32)];
    self.voice.imagePosition = NSImageLeading;
    self.commit.wantsLayer = YES;
    self.commit.layer.cornerRadius = 8;
    self.commit.layer.backgroundColor = [NSColor colorWithSRGBRed:142/255.0 green:245/255.0 blue:104/255.0 alpha:1].CGColor;
    self.dictation = [KyDictation new];
}
- (void)openAt:(NSButton *)button english:(BOOL)english locked:(BOOL)locked {
    if (!self.panel) [self buildPanel];
    self.english = english;
    self.heading.stringValue = english ? @"Quick note" : @"速记";
    self.panel.title = self.heading.stringValue;
    self.body.accessibilityLabel = english ? @"Note text" : @"速记正文";
    self.hide.title = english ? @"Hide" : @"收起";
    self.commit.attributedTitle = [[NSAttributedString alloc] initWithString:english ? @"Save ⌘↩" : @"保存 ⌘↩"
        attributes:@{NSForegroundColorAttributeName:NSColor.blackColor, NSFontAttributeName:[NSFont boldSystemFontOfSize:13]}];
    self.pasteButton.title = english ? @"Paste" : @"粘贴";
    self.pasteButton.toolTip = english ? @"Paste text or an image (⌘V)" : @"粘贴文字或图片（⌘V）";
    self.attachment.title = english ? @"Remove image" : @"移除图片";
    self.voice.toolTip = english ? @"Transcribe on this Mac" : @"在这台 Mac 上将语音转为文字";
    if (!self.dictation.active) {
        [self setListening:NO];
        self.status.stringValue = [self hint];
    }
    if (locked) {
        [self clearDraft];
        self.status.stringValue = english ? @"Unlock Ky before taking a note." : @"请先解锁 Ky，再记速记。";
    }
    self.body.editable = self.pasteButton.enabled = self.voice.enabled = self.commit.enabled = !locked;
    NSScreen *screen = button.window.screen ?: NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    NSRect anchor = button.window.screen ? [button.window convertRectToScreen:[button convertRect:button.bounds toView:nil]]
                                  : NSMakeRect(NSMaxX(visible)-24, NSMaxY(visible), 0, 0);
    NSSize size = self.panel.frame.size;
    CGFloat x = MIN(NSMaxX(visible)-size.width, NSMidX(anchor)-size.width/2);
    CGFloat y = MIN(NSMaxY(visible), NSMinY(anchor))-size.height-4;
    [self.panel setFrameOrigin:NSMakePoint(MAX(NSMinX(visible), x), MAX(NSMinY(visible), y))];
    [self.panel makeKeyAndOrderFront:nil];
    [self.panel makeFirstResponder:locked ? (NSResponder *)self.hide : self.body];
}
- (void)pasteNote:(id)sender {
    [self.panel makeFirstResponder:self.body];
    [self.body paste:sender];
}
- (void)setListening:(BOOL)active {
    self.voice.title = active ? (self.english ? @"Finish" : @"结束") : [self voiceTitle];
    self.voice.image = [NSImage imageWithSystemSymbolName:active ? @"stop.circle.fill" : @"mic" accessibilityDescription:nil];
}
- (void)noteActivity {
    if (self.activity) self.activity();
}
- (void)textDidChange:(NSNotification *)notification { [self noteActivity]; }
- (void)dictate:(id)sender {
    [self noteActivity];
    [self.panel makeKeyWindow];
    [self.panel makeFirstResponder:self.body];
    if (self.dictation.active) { [self.dictation finishListening]; return; }
    __weak KyFlash *weakSelf = self;
    [self.dictation startInEditor:self.body english:self.english status:^(NSString *message, BOOL active) {
        KyFlash *owner = weakSelf;
        owner.status.stringValue = message;
        if (active) [owner noteActivity];
        [owner setListening:active];
    }];
}
- (void)pasteImage:(NSPasteboard *)board type:(NSPasteboardType)type {
    if (self.image.length) {
        self.status.stringValue = self.english ? @"Remove the attached image before pasting another." : @"先移除已附图片，再粘贴另一张。";
        return;
    }
    NSData *bytes = [board dataForType:type];
    const NSUInteger limit = 8 * 1024 * 1024;
    NSString *mime = @"image/png";
    if ([type isEqual:UTTypeJPEG.identifier]) mime = @"image/jpeg";
    else if ([type isEqual:UTTypeWebP.identifier]) mime = @"image/webp";
    else if ([type isEqual:NSPasteboardTypeTIFF] && bytes.length && bytes.length <= limit) {
        NSBitmapImageRep *rep = [NSBitmapImageRep imageRepWithData:bytes];
        bytes = rep && rep.pixelsWide * (double)rep.pixelsHigh <= 20000000
            ? [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}] : nil;
    }
    if (!bytes.length || bytes.length > limit) {
        self.status.stringValue = self.english ? @"Paste a smaller image (8 MiB maximum)." : @"请粘贴较小的图片（最多 8 MiB）。";
        return;
    }
    self.image = [NSString stringWithFormat:@"data:%@;base64,%@",mime,[bytes base64EncodedStringWithOptions:0]];
    [self noteActivity];
    self.attachment.hidden = NO;
    self.status.stringValue = self.english ? @"One image attached" : @"已附一张图片";
}
- (void)removeImage:(id)sender {
    self.image = nil;
    self.attachment.hidden = YES;
    self.status.stringValue = [self hint];
    [self.panel makeFirstResponder:self.body];
}
- (void)clearDraft {
    [self.dictation stop];
    [self.body.inputContext discardMarkedText];
    self.body.string = @"";
    [self.body.undoManager removeAllActions];
    self.image = nil;
    self.attachment.hidden = YES;
    self.status.stringValue = [self hint];
    self.status.toolTip = nil;
}
- (void)dismiss {
    [self.dictation stop];
    self.status.stringValue = [self hint];
    self.status.toolTip = nil;
    [self.panel orderOut:nil];
}
- (void)windowDidResignKey:(NSNotification *)notification {
    if (!self.dictation.awaitingAuthorization) [self dismiss];
}
- (NSString *)saveText:(NSString *)text image:(NSString *)image {
    if (![text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length && !image.length)
        return self.english ? @"Enter text or paste an image first." : @"请先输入文字或粘贴图片。";
    if ([text containsString:[NSString stringWithCharacters:(unichar[]){0} length:1]])
        return self.english ? @"Remove null characters before saving." : @"请移除正文中的空字符后再保存。";
    const char *utf8 = text.UTF8String;
    if (!utf8 || !self.save || !self.save(utf8, image.UTF8String ?: ""))
        return self.english ? @"Could not save. Check the content and retry." : @"未能保存，请检查内容再试。";
    return nil;
}
- (void)submit:(id)sender {
    [self.dictation stop];
    if (self.body.hasMarkedText) [self.body unmarkText];
    NSString *error = [self saveText:self.body.string image:self.image];
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
    onMain(^{ message = [self saveText:[pasteboard stringForType:NSPasteboardTypeString] image:nil]; });
    if (error) *error = message;
}
@end

static KyFlash *provider;

void ky_flash_setup(KySave save, KyActivity activity) {
    onMain(^{
        if (!provider) provider = [KyFlash new];
        provider.save = save;
        provider.activity = activity;
        [NSApplication sharedApplication].servicesProvider = provider;
        NSUpdateDynamicServices();
    });
}

void ky_flash_open(const void *item, bool english, bool locked) {
    onMain(^{ [provider openAt:[(__bridge NSStatusItem *)item button] english:english locked:locked]; });
}

void ky_flash_lock(void) {
    onMain(^{
        [provider dismiss];
        [provider clearDraft];
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
