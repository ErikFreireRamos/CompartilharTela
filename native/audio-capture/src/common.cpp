#include "common.h"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <cwctype>
#include <mutex>

namespace {

std::mutex g_outputMutex;

void SendFrame(uint8_t type, const void* data, uint32_t size) {
  std::vector<uint8_t> frame(5 + size);
  frame[0] = type;
  frame[1] = static_cast<uint8_t>(size);
  frame[2] = static_cast<uint8_t>(size >> 8);
  frame[3] = static_cast<uint8_t>(size >> 16);
  frame[4] = static_cast<uint8_t>(size >> 24);
  memcpy(frame.data() + 5, data, size);

  std::lock_guard<std::mutex> lock(g_outputMutex);
  HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
  const uint8_t* p = frame.data();
  DWORD remaining = static_cast<DWORD>(frame.size());
  while (remaining > 0) {
    DWORD written = 0;
    // o app fechou o pipe: não há mais para quem enviar
    if (!WriteFile(out, p, remaining, &written, nullptr) || written == 0) ExitProcess(0);
    p += written;
    remaining -= written;
  }
}

size_t FindValue(const std::string& json, const char* key) {
  const std::string needle = std::string("\"") + key + "\"";
  size_t pos = json.find(needle);
  if (pos == std::string::npos) return pos;
  pos = json.find(':', pos + needle.size());
  if (pos == std::string::npos) return pos;
  for (++pos; pos < json.size() && std::isspace(static_cast<unsigned char>(json[pos])); ++pos) {
  }
  return pos < json.size() ? pos : std::string::npos;
}

void AppendUtf8(std::string& out, unsigned codepoint) {
  if (codepoint < 0x80) {
    out += static_cast<char>(codepoint);
  } else if (codepoint < 0x800) {
    out += static_cast<char>(0xC0 | (codepoint >> 6));
    out += static_cast<char>(0x80 | (codepoint & 0x3F));
  } else {
    out += static_cast<char>(0xE0 | (codepoint >> 12));
    out += static_cast<char>(0x80 | ((codepoint >> 6) & 0x3F));
    out += static_cast<char>(0x80 | (codepoint & 0x3F));
  }
}

// json[pos] deve ser '"'; avança pos até depois da aspa final.
std::string ParseString(const std::string& json, size_t& pos) {
  std::string out;
  for (++pos; pos < json.size() && json[pos] != '"'; ++pos) {
    if (json[pos] != '\\' || pos + 1 >= json.size()) {
      out += json[pos];
      continue;
    }
    const char c = json[++pos];
    switch (c) {
      case 'n': out += '\n'; break;
      case 't': out += '\t'; break;
      case 'r': out += '\r'; break;
      case 'u':
        if (pos + 4 < json.size()) {
          AppendUtf8(out, std::strtoul(json.substr(pos + 1, 4).c_str(), nullptr, 16));
          pos += 4;
        }
        break;
      default: out += c;
    }
  }
  ++pos;
  return out;
}

}  // namespace

void SendJson(const std::string& json) { SendFrame(2, json.data(), static_cast<uint32_t>(json.size())); }

void SendPcm(const float* samples, size_t frames) {
  SendFrame(1, samples, static_cast<uint32_t>(frames * kChannels * sizeof(float)));
}

void Log(const std::string& message) { SendJson("{\"event\":\"log\",\"message\":" + JsonString(message) + "}"); }

std::string ToUtf8(const std::wstring& text) {
  if (text.empty()) return {};
  const int size = WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0, nullptr, nullptr);
  std::string out(size, '\0');
  WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), out.data(), size, nullptr, nullptr);
  return out;
}

std::wstring FromUtf8(const std::string& text) {
  if (text.empty()) return {};
  const int size = MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0);
  std::wstring out(size, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), out.data(), size);
  return out;
}

std::wstring ToLower(std::wstring text) {
  std::transform(text.begin(), text.end(), text.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
  return text;
}

std::string JsonString(const std::string& utf8) {
  std::string out = "\"";
  for (const char c : utf8) {
    if (c == '"' || c == '\\') {
      out += '\\';
      out += c;
    } else if (static_cast<unsigned char>(c) < 0x20) {
      char buf[8];
      snprintf(buf, sizeof(buf), "\\u%04x", static_cast<unsigned char>(c));
      out += buf;
    } else {
      out += c;
    }
  }
  return out + "\"";
}

std::string JsonGetString(const std::string& json, const char* key) {
  size_t pos = FindValue(json, key);
  if (pos == std::string::npos || json[pos] != '"') return {};
  return ParseString(json, pos);
}

long long JsonGetInt(const std::string& json, const char* key, long long fallback) {
  const size_t pos = FindValue(json, key);
  if (pos == std::string::npos) return fallback;
  char* end = nullptr;
  const long long value = std::strtoll(json.c_str() + pos, &end, 10);
  return end == json.c_str() + pos ? fallback : value;
}

std::vector<std::string> JsonGetStringArray(const std::string& json, const char* key) {
  std::vector<std::string> out;
  size_t pos = FindValue(json, key);
  if (pos == std::string::npos || json[pos] != '[') return out;
  ++pos;
  while (pos < json.size()) {
    const char c = json[pos];
    if (c == ']') break;
    if (c == '"') {
      out.push_back(ParseString(json, pos));
    } else {
      ++pos;
    }
  }
  return out;
}
