// Unreal face of a simulation: ticks at a fixed rate, keeps one actor per entity.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "SimcraftWorld.generated.h"

class USimcraftSimulation;

DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FSimcraftBusMessages, const FString&, JsonArray);

UCLASS(Blueprintable)
class SIMCRAFT_API ASimcraftWorld : public AActor
{
    GENERATED_BODY()

public:
    ASimcraftWorld();

    /** Folder with game.ron and engine.toml, relative to the project's Content directory. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Simcraft")
    FString GameFolder = TEXT("Games/wolf_sheep");

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Simcraft", meta = (ClampMin = "0.01"))
    float TicksPerSecond = 10.f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Simcraft")
    bool bPlayOnStart = true;

    /** World units per grid cell. Grid x -> X, grid y -> -Y. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Simcraft")
    float CellSize = 100.f;

    /** Actor class per kind. A kind without one is not shown. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Simcraft")
    TMap<FString, TSubclassOf<AActor>> KindActors;

    /** Every bus message since the last frame (JSON array). */
    UPROPERTY(BlueprintAssignable, Category = "Simcraft")
    FSimcraftBusMessages OnBusMessages;

    UPROPERTY(BlueprintReadOnly, Category = "Simcraft")
    TObjectPtr<USimcraftSimulation> Simulation;

    UPROPERTY(BlueprintReadWrite, Category = "Simcraft")
    bool bPlaying = false;

    /** Advance by hand (turn-based games with bPlayOnStart off). */
    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    void StepOnce(int32 N = 1);

    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    FString SaveGame();

    UFUNCTION(BlueprintCallable, Category = "Simcraft")
    bool LoadGame(const FString& Save, FString& OutError);

protected:
    virtual void BeginPlay() override;
    virtual void Tick(float DeltaSeconds) override;

private:
    void Sync();

    struct FView
    {
        TWeakObjectPtr<AActor> Actor;
        FString Glyph;
    };
    TMap<int64, FView> Live;
    float Clock = 0.f;
};
