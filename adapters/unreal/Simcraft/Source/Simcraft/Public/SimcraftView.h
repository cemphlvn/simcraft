// Implement on a kind's actor to react to the simulation. The core decides; the view only shows.
#pragma once

#include "CoreMinimal.h"
#include "UObject/Interface.h"
#include "SimcraftView.generated.h"

UINTERFACE(BlueprintType)
class SIMCRAFT_API USimcraftView : public UInterface
{
    GENERATED_BODY()
};

class SIMCRAFT_API ISimcraftView
{
    GENERATED_BODY()

public:
    /** Once, when the entity appears. */
    UFUNCTION(BlueprintNativeEvent, Category = "Simcraft")
    void OnSimcraftSpawn(int64 Id, const FString& Kind);

    /** When the state's glyph changes (the designer maps states to glyphs in game.ron). */
    UFUNCTION(BlueprintNativeEvent, Category = "Simcraft")
    void OnSimcraftState(const FString& Glyph);
};
