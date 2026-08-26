import Foundation
@testable import NetStatus
import Testing

private let probeURL = URL(string: "http://captive.apple.com/hotspot-detect.html")!

private func httpResponse(_ status: Int) -> HTTPURLResponse {
    HTTPURLResponse(url: probeURL, statusCode: status, httpVersion: "HTTP/1.1", headerFields: nil)!
}

private func fetching(status: Int, body: String) -> @Sendable (URL) async throws -> (Data, URLResponse) {
    { _ in (Data(body.utf8), httpResponse(status)) }
}

private struct TransportFailure: Error {}

struct CaptivePortalTests {
    @Test func exactSuccessBodyIsClear() {
        #expect(
            Probe.classifyCaptivePortalResponse(
                status: 200,
                body: "<HTML><HEAD><TITLE>Success</TITLE></HEAD><BODY>Success</BODY></HTML>"
            ) == .clear
        )
    }

    @Test func bareSuccessWithoutHtmlWrapperIsClear() {
        #expect(Probe.classifyCaptivePortalResponse(status: 200, body: "Success") == .clear)
    }

    @Test func alteredBodyIsDetected() {
        #expect(
            Probe.classifyCaptivePortalResponse(
                status: 200, body: "<HTML><BODY>Login required</BODY></HTML>"
            ) == .detected
        )
    }

    @Test func non200StatusIsDetected() {
        #expect(Probe.classifyCaptivePortalResponse(status: 302, body: "Success") == .detected)
    }

    @Test func successResponseIsClear() async {
        let status = await Probe.checkCaptivePortal(fetch: fetching(status: 200, body: "Success"))
        #expect(status == .clear)
    }

    @Test func rewrittenBodyIsDetected() async {
        let status = await Probe.checkCaptivePortal(fetch: fetching(status: 200, body: "<a>sign in</a>"))
        #expect(status == .detected)
    }

    @Test func non200IsDetected() async {
        let status = await Probe.checkCaptivePortal(fetch: fetching(status: 511, body: "Success"))
        #expect(status == .detected)
    }

    @Test func transportErrorIsUnknown() async {
        let status = await Probe.checkCaptivePortal(fetch: { _ in throw TransportFailure() })
        #expect(status == .unknown)
    }

    @Test func nonHttpResponseIsUnknown() async {
        let status = await Probe.checkCaptivePortal(fetch: { url in
            (Data(), URLResponse(url: url, mimeType: nil, expectedContentLength: 0, textEncodingName: nil))
        })
        #expect(status == .unknown)
    }
}

@Suite(.live, .tags(.network), .timeLimit(.minutes(1)))
struct CaptivePortalLiveTests {
    @Test func liveProbeIsConclusive() async {
        let status = await Probe.checkCaptivePortal()
        #expect(status == .clear || status == .detected || status == .unknown)
    }
}
