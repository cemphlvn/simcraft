// Header-only C++17 wrapper for simcraft.h. No Unreal types: usable from any C++ host.
#pragma once

#include <cstdint>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "simcraft.h"

namespace simcraft {

// The JSON the core returned, e.g. {"ok":false,"stage":"validate","errors":[...]}.
struct Error : std::runtime_error {
    using std::runtime_error::runtime_error;
};

namespace detail {
inline std::string take(char* s) {
    if (!s) return {};
    std::string out(s);
    simcraft_string_free(s);
    return out;
}
}  // namespace detail

class Sim {
public:
    static Sim load(const std::string& game_ron, const std::string& engine_toml) {
        if (simcraft_abi_version() != SIMCRAFT_ABI_VERSION) throw Error("simcraft: native library ABI mismatch");
        char* err = nullptr;
        SimcraftSim* h = simcraft_new(game_ron.c_str(), engine_toml.c_str(), &err);
        if (!h) throw Error(detail::take(err));
        return Sim(h);
    }

    Sim(Sim&& o) noexcept : h_(std::exchange(o.h_, nullptr)) {}
    Sim& operator=(Sim&& o) noexcept {
        if (this != &o) {
            simcraft_free(h_);
            h_ = std::exchange(o.h_, nullptr);
        }
        return *this;
    }
    Sim(const Sim&) = delete;
    Sim& operator=(const Sim&) = delete;
    ~Sim() { simcraft_free(h_); }

    // Any agent-protocol request → JSON reply.
    std::string request(const std::string& json) { return detail::take(simcraft_request(h_, json.c_str())); }

    int64_t step(uint32_t n = 1) { return simcraft_step(h_, n); }

    // Every entity this tick (buffer reused between calls).
    const std::vector<SimcraftEntity>& entities() {
        size_t total = simcraft_entities(h_, buf_.data(), buf_.size());
        if (total > buf_.size()) {
            buf_.resize(total * 2);
            total = simcraft_entities(h_, buf_.data(), buf_.size());
        }
        view_.assign(buf_.begin(), buf_.begin() + static_cast<std::ptrdiff_t>(total));
        return view_;
    }

    std::string kind_name(uint32_t kind) const {
        const char* s = simcraft_kind_name(h_, kind);
        return s ? s : "";
    }

    // Every bus message since the last call (JSON array).
    std::string drain() { return detail::take(simcraft_drain(h_)); }

    // The whole game at this tick: keep it as the save file.
    std::string save() { return request(R"({"cmd":"snapshot"})"); }

    // Back to a save; the future is bit-identical. Throws if the save is from another game.
    void load_save(const std::string& save) {
        auto open = save.find('{');
        if (open == std::string::npos) throw Error("simcraft: a save is a JSON object");
        std::string reply = request(R"({"cmd":"restore",)" + save.substr(open + 1));
        if (reply.find("\"ok\":true") == std::string::npos) throw Error(reply);
    }

private:
    explicit Sim(SimcraftSim* h) : h_(h) {}
    SimcraftSim* h_ = nullptr;
    std::vector<SimcraftEntity> buf_ = std::vector<SimcraftEntity>(64);
    std::vector<SimcraftEntity> view_;
};

}  // namespace simcraft
