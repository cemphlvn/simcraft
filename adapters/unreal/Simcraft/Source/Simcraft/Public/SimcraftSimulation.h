// One running game, callable from Blueprints. The core decides; Unreal displays.
#pragma once

#include "CoreMinimal.h"
#include "UObject/Object.h"
#include "SimcraftSimulation.generated.h"

struct SimcraftSim;

USTRUCT(BlueprintType)
struct FSimcraftEntity
{
    GENERATED_BODY()

    UPROPERTY(BlueprintReadOnly, Category = "Simcraft") int64 Id = 0;
    /** Grid cell. */
    UPROPERTY(BlueprintReadOnly, Category = "Simcraft") int64 X = 0;
    UPROPERTY(BlueprintReadOnly, Category = "Simcraft") int64 Y = 0;
    UPROPERTY(BlueprintReadOnly, Category = "Simcraft") FString Kind;
    /** The current state's glyph: the designer's state -> look map in game.ron. */
    UPROPERTY(BlueprintReadOnly, Category = "Simcraft") FString Glyph;
};

UCLASS(BlueprintType)
class SIMCRAFT_API USimcraftSimulation : public UObject
{
    GENERATED_BODY()

public:
    virtual void BeginDestroy() override;

    /** Load a game from the text of game.ron and engine.toml. On failure OutError holds every error (JSON). */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    bool Load(const FString& GameRon, const FString& EngineToml, FString& OutError);

    /** Load from a folder holding game.ron and engine.toml (e.g. Content/Games/wolf_sheep). */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    bool LoadFolder(const FString& Folder, FString& OutError);

    /** Advance up to N ticks; returns the tick (-1 if nothing is loaded). */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    int64 Step(int32 N = 1);

    /** Any agent-protocol request (info, observe, act, step, hash, snapshot, restore) -> JSON reply. */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    FString Request(const FString& Json);

    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    TArray<FSimcraftEntity> GetEntities();

    /** Every bus message since the last call, as a JSON array. */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    FString Drain();

    /** The whole game at this tick. Keep the string as the save file. */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    FString Save();

    /** Back to a save; the future is bit-identical. False (and nothing changes) if the save is from another game. */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    bool LoadSave(const FString& SaveJson, FString& OutError);

    UFUNCTION(BlueprintPure, Category = "Simcraft")
    bool IsLoaded() const { return Sim != nullptr; }

private:
    SimcraftSim* Sim = nullptr;
    TArray<FString> KindNames;
};
