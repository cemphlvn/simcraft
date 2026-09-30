// The whole app is sim-mobile (a Rust static library): winit takes over UIApplicationMain from here.
#import <QuartzCore/QuartzCore.h>

extern void simcraft_mobile_main(void);

// ProMotion: iOS runs the screen at 120 Hz only while something asks for it. This display link asks (80–120 Hz,
// preferably 120) and does nothing else; winit's redraws then follow the faster screen. Info.plist's
// CADisableMinimumFrameDurationOnPhone allows it; phones without ProMotion stay at 60.
@interface SimcraftPace : NSObject
- (void)tick:(CADisplayLink *)link;
@end

@implementation SimcraftPace
- (void)tick:(CADisplayLink *)link {
}
@end

int main(int argc, char *argv[]) {
    @autoreleasepool {
        CADisplayLink *link = [CADisplayLink displayLinkWithTarget:[SimcraftPace new] selector:@selector(tick:)];
        if (@available(iOS 15.0, *)) {
            link.preferredFrameRateRange = CAFrameRateRangeMake(80, 120, 120);
        }
        [link addToRunLoop:[NSRunLoop mainRunLoop] forMode:NSRunLoopCommonModes];
    }
    simcraft_mobile_main();
    return 0;
}
