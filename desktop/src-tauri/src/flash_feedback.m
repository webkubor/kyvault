#import <AppKit/AppKit.h>

// The HUD never activates Ky, intercepts clicks, or displays captured content.
static NSPanel *hud;
static NSUInteger generation;
static NSTimeInterval lastSound = -10;

bool ky_flash_sound(bool update, bool enabled) {
    NSUserDefaults *prefs = NSUserDefaults.standardUserDefaults;
    NSString *key = @"KyFlashSoundMuted";
    if (update) [prefs setBool:!enabled forKey:key];
    return ![prefs boolForKey:key];
}

static NSPanel *makeHUD(NSString *message) {
    NSPanel *panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0,0,202,44)
        styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
        backing:NSBackingStoreBuffered defer:NO];
    panel.backgroundColor = NSColor.clearColor;
    panel.opaque = NO;
    panel.hasShadow = YES;
    panel.ignoresMouseEvents = YES;
    panel.hidesOnDeactivate = NO;
    panel.level = NSStatusWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    NSVisualEffectView *surface = [[NSVisualEffectView alloc] initWithFrame:panel.contentView.bounds];
    surface.material = NSVisualEffectMaterialHUDWindow;
    surface.state = NSVisualEffectStateActive;
    surface.wantsLayer = YES;
    surface.layer.cornerRadius = 13;
    surface.layer.masksToBounds = YES;
    panel.contentView = surface;
    NSImageView *check = [[NSImageView alloc] initWithFrame:NSMakeRect(14,12,20,20)];
    check.image = [NSImage imageWithSystemSymbolName:@"checkmark.circle.fill" accessibilityDescription:nil];
    check.contentTintColor = NSColor.systemGreenColor;
    [surface addSubview:check];
    NSTextField *label = [NSTextField labelWithString:message];
    label.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    [label sizeToFit];
    label.frameOrigin = NSMakePoint(42,round((44-label.frame.size.height)/2));
    [surface addSubview:label];
    return panel;
}

void ky_flash_received(bool overlay, bool audible, bool english) {
    dispatch_async(dispatch_get_main_queue(), ^{
        NSTimeInterval now = NSProcessInfo.processInfo.systemUptime;
        if (audible && ky_flash_sound(false,false) && now-lastSound > 0.8) {
            NSSound *drop = [NSSound soundNamed:@"Pop"];
            drop.volume = 0.32;
            [drop play];
            lastSound = now;
        }
        if (!overlay) return;
        NSString *message = english ? @"Saved to private notes" : @"已存到私密收藏";
        BOOL reduced = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
        [hud orderOut:nil];
        hud = makeHUD(message);
        NSUInteger current = ++generation;
        NSRect screen = NSScreen.mainScreen.visibleFrame;
        [hud setFrameOrigin:NSMakePoint(NSMaxX(screen)-hud.frame.size.width-20, NSMaxY(screen)-60)];
        hud.alphaValue = reduced ? 1 : 0;
        [hud orderFrontRegardless];
        if (!reduced) [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
            ctx.duration = 0.13; hud.animator.alphaValue = 1;
        } completionHandler:nil];
        NSAccessibilityPostNotificationWithUserInfo(hud, NSAccessibilityAnnouncementRequestedNotification,
            @{NSAccessibilityAnnouncementKey:message,NSAccessibilityPriorityKey:@(NSAccessibilityPriorityMedium)});
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1500*NSEC_PER_MSEC),dispatch_get_main_queue(), ^{
            if (current != generation) return;
            if (reduced) { [hud orderOut:nil]; return; }
            [NSAnimationContext runAnimationGroup:^(NSAnimationContext *ctx) {
                ctx.duration = 0.18; hud.animator.alphaValue = 0;
            } completionHandler:^{ if (current == generation) [hud orderOut:nil]; }];
        });
    });
}
