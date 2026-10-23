#pragma once

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <cstdint>
#include <string>
#include <vector>

constexpr int kSampleRate = 48000;
constexpr int kChannels = 2;
constexpr size_t kChunkFrames = 480;  // 10 ms

// Saída para o app pelo stdout: [tipo u8][tamanho u32 LE][dados]
// tipo 1 = PCM float32 estéreo intercalado, tipo 2 = JSON (UTF-8)
void SendJson(const std::string& json);
void SendPcm(const float* samples, size_t frames);
void Log(const std::string& message);

std::string ToUtf8(const std::wstring& text);
std::wstring FromUtf8(const std::string& text);
std::wstring ToLower(std::wstring text);
/** Texto UTF-8 entre aspas, com escape para JSON. */
std::string JsonString(const std::string& utf8);

// Leitura mínima dos comandos JSON (uma linha por comando, gerados pelo app).
std::string JsonGetString(const std::string& json, const char* key);
long long JsonGetInt(const std::string& json, const char* key, long long fallback);
std::vector<std::string> JsonGetStringArray(const std::string& json, const char* key);
