#include "SimcraftSimulation.h"

#include "Misc/FileHelper.h"
#include "Misc/Paths.h"
#include "simcraft.h"

namespace
{
FString Take(char* S)
{
    if (!S) return FString();
    FString Out = UTF8_TO_TCHAR(S);
    simcraft_string_free(S);
    return Out;
}

template <typename F> void ForEachEntity(SimcraftSim* Sim, F&& Visit)
{
    if (!Sim) return;
    static thread_local TArray<SimcraftEntity> Buffer;
    size_t Total = simcraft_entities(Sim, Buffer.GetData(), Buffer.Num());
    if (Total > static_cast<size_t>(Buffer.Num()))
    {
        Buffer.SetNumUninitialized(static_cast<int32>(Total * 2));
        Total = simcraft_entities(Sim, Buffer.GetData(), Buffer.Num());
    }
    for (size_t I = 0; I < Total; ++I) Visit(Buffer[I]);
}
}  // namespace

void USimcraftSimulation::BeginDestroy()
{
    simcraft_free(Sim);
    Sim = nullptr;
    Super::BeginDestroy();
}

bool USimcraftSimulation::Load(const FString& GameRon, const FString& EngineToml, FString& OutError)
{
    if (simcraft_abi_version() != SIMCRAFT_ABI_VERSION)
    {
        OutError = TEXT("{\"ok\":false,\"error\":\"native library ABI mismatch\"}");
        return false;
    }
    simcraft_free(Sim);
    KindNames.Reset();
    char* Err = nullptr;
    Sim = simcraft_new(TCHAR_TO_UTF8(*GameRon), TCHAR_TO_UTF8(*EngineToml), &Err);
    if (!Sim)
    {
        OutError = Take(Err);
        return false;
    }
    for (uint32 K = 0;; ++K)
    {
        const char* Name = simcraft_kind_name(Sim, K);
        if (!Name) break;
        KindNames.Add(UTF8_TO_TCHAR(Name));
    }
    return true;
}

bool USimcraftSimulation::LoadFolder(const FString& Folder, FString& OutError)
{
    FString Game, Panel;
    if (!FFileHelper::LoadFileToString(Game, *FPaths::Combine(Folder, TEXT("game.ron"))) ||
        !FFileHelper::LoadFileToString(Panel, *FPaths::Combine(Folder, TEXT("engine.toml"))))
    {
        OutError = FString::Printf(TEXT("{\"ok\":false,\"error\":\"cannot read game.ron/engine.toml in %s\"}"), *Folder);
        return false;
    }
    return Load(Game, Panel, OutError);
}

int64 USimcraftSimulation::Step(int32 N)
{
    return Sim ? simcraft_step(Sim, static_cast<uint32>(FMath::Max(N, 0))) : -1;
}

FString USimcraftSimulation::Request(const FString& Json)
{
    if (!Sim) return TEXT("{\"ok\":false,\"error\":\"nothing loaded\"}");
    return Take(simcraft_request(Sim, TCHAR_TO_UTF8(*Json)));
}

TArray<FSimcraftEntity> USimcraftSimulation::GetEntities()
{
    TArray<FSimcraftEntity> Out;
    ForEachEntity(Sim, [&](const SimcraftEntity& E) {
        FSimcraftEntity& R = Out.AddDefaulted_GetRef();
        R.Id = static_cast<int64>(E.id);
        R.X = E.x;
        R.Y = E.y;
        R.Kind = KindNames.IsValidIndex(E.kind) ? KindNames[E.kind] : FString();
        R.Glyph = FString::Chr(static_cast<TCHAR>(E.glyph));
    });
    return Out;
}

FString USimcraftSimulation::Drain()
{
    return Sim ? Take(simcraft_drain(Sim)) : TEXT("[]");
}

FString USimcraftSimulation::Save()
{
    return Request(TEXT("{\"cmd\":\"snapshot\"}"));
}

bool USimcraftSimulation::LoadSave(const FString& SaveJson, FString& OutError)
{
    int32 Open = INDEX_NONE;
    if (!SaveJson.FindChar(TEXT('{'), Open))
    {
        OutError = TEXT("{\"ok\":false,\"error\":\"a save is a JSON object\"}");
        return false;
    }
    const FString Reply = Request(TEXT("{\"cmd\":\"restore\",") + SaveJson.Mid(Open + 1));
    if (!Reply.Contains(TEXT("\"ok\":true")))
    {
        OutError = Reply;
        return false;
    }
    return true;
}
