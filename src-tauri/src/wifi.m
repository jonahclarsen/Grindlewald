#import <CoreLocation/CoreLocation.h>
#import <CoreWLAN/CoreWLAN.h>
#import <Foundation/Foundation.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

@interface GrindlewaldLocationDelegate : NSObject <CLLocationManagerDelegate>
@end
@implementation GrindlewaldLocationDelegate
- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
    // The UI refreshes while permission is pending and when the app regains focus.
}
@end

// Called from a Rust blocking worker. Keep Core Location on the main run loop,
// and retain its manager/delegate for the lifetime of the app. Never start GPS updates.
int grindlewald_wifi_snapshot(bool request_permission, char **ssid) {
    __block int status = 1;
    __block char *network = NULL;
    void (^read)(void) = ^{
        @autoreleasepool {
            static CLLocationManager *manager;
            static GrindlewaldLocationDelegate *delegate;
            if (!manager) {
                delegate = [GrindlewaldLocationDelegate new];
                manager = [CLLocationManager new];
                manager.delegate = delegate;
            }
            if (![CLLocationManager locationServicesEnabled]) {
                status = 4;
                return;
            }
            switch (manager.authorizationStatus) {
                case kCLAuthorizationStatusNotDetermined:
                    if (request_permission) [manager requestAlwaysAuthorization];
                    status = 2;
                    return;
                case kCLAuthorizationStatusDenied:
                    status = 3;
                    return;
                case kCLAuthorizationStatusRestricted:
                    status = 5;
                    return;
                default:
                    break;
            }
            // Discover the actual Wi-Fi interface instead of assuming en0.
            for (CWInterface *interface in [CWWiFiClient sharedWiFiClient].interfaces) {
                NSString *name = interface.ssid;
                if (name.length && ![name isEqualToString:@"<redacted>"]) {
                    network = strdup(name.UTF8String);
                    status = network ? 0 : 6;
                    return;
                }
            }
        }
    };
    if ([NSThread isMainThread]) read();
    else dispatch_sync(dispatch_get_main_queue(), read);
    *ssid = network;
    return status;
}
