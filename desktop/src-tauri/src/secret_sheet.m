#import <AppKit/AppKit.h>

static NSAlert *visibleAlert;

void ky_hide_secret(void) {
    NSWindow *sheet = visibleAlert.window;
    if (sheet.sheetParent) [sheet.sheetParent endSheet:sheet];
}

bool ky_show_secret(void *rawWindow, const unsigned char *bytes, size_t length, bool english) {
    NSWindow *parent = (__bridge NSWindow *)rawWindow;
    if (![NSThread isMainThread] || !parent.isKeyWindow || parent.attachedSheet) return false;
    NSAlert *alert = [NSAlert new];
    alert.messageText = english ? @"Password or key" : @"密码或密钥";
    alert.informativeText = english ? @"Hidden after 30 seconds." : @"30 秒后自动隐藏。";
    [alert addButtonWithTitle:english ? @"Hide" : @"隐藏"];
    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(0, 0, 360, 90)];
    NSTextView *text = [[NSTextView alloc] initWithFrame:scroll.bounds];
    text.editable = NO;
    text.selectable = NO;
    text.string = [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
    scroll.documentView = text;
    scroll.hasVerticalScroller = YES;
    alert.accessoryView = scroll;
    visibleAlert = alert;
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    id observer = [center addObserverForName:NSWindowDidResignKeyNotification object:alert.window queue:nil
        usingBlock:^(NSNotification *__unused note) { if (visibleAlert == alert) ky_hide_secret(); }];
    [alert beginSheetModalForWindow:parent completionHandler:^(NSModalResponse __unused response) {
        [center removeObserver:observer];
        text.string = @"";
        if (visibleAlert == alert) visibleAlert = nil;
    }];
    __weak NSAlert *presented = alert;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        if (visibleAlert && visibleAlert == presented) ky_hide_secret();
    });
    return true;
}
