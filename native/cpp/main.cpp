#include <iostream>
#include <string>
#include <unistd.h>

// v0.2 deliberately keeps the C++ worker narrow. It does not execute model-provided commands.
// Future Linux primitives will be exposed through explicit, typed operations only.

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);

    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.size() > 256 * 1024) {
            std::cout << R"({"version":1,"ok":false,"request_id":"","error":"request too large"})" << '\n' << std::flush;
            continue;
        }

        // The first C++ operation is intentionally a health probe. JSON parsing is kept
        // out of this layer until the protocol is promoted to a stable shared component.
        if (line.find("\"op\":\"health\"") != std::string::npos) {
            std::cout << R"({"version":1,"ok":true,"request_id":"","result":{"component":"cpp-system","pid":)"
                      << static_cast<long long>(getpid())
                      << "}}\n" << std::flush;
        } else {
            std::cout << R"({"version":1,"ok":false,"request_id":"","error":"unsupported operation"})" << '\n' << std::flush;
        }
    }
    return 0;
}
