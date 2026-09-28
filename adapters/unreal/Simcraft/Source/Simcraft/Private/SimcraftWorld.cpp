#include "SimcraftWorld.h"

#include "Engine/World.h"
#include "Misc/Paths.h"
#include "SimcraftSimulation.h"
#include "SimcraftView.h"

ASimcraftWorld::ASimcraftWorld()
{
    PrimaryActorTick.bCanEverTick = true;
}

void ASimcraftWorld::BeginPlay()
{
    Super::BeginPlay();
    Simulation = NewObject<USimcraftSimulation>(this);
    FString Error;
    if (!Simulation->LoadFolder(FPaths::Combine(FPaths::ProjectContentDir(), GameFolder), Error))
    {
        UE_LOG(LogTemp, Error, TEXT("simcraft could not load %s: %s"), *GameFolder, *Error);
        SetActorTickEnabled(false);
        return;
    }
    bPlaying = bPlayOnStart;
    Sync();
}

void ASimcraftWorld::Tick(float DeltaSeconds)
{
    Super::Tick(DeltaSeconds);
    if (!bPlaying || !Simulation || !Simulation->IsLoaded()) return;
    Clock += DeltaSeconds;
    const float Dt = 1.f / TicksPerSecond;
    int32 N = 0;
    while (Clock >= Dt && N < 64)
    {
        Clock -= Dt;
        ++N;
    }
    if (N > 0) StepOnce(N);
}

void ASimcraftWorld::StepOnce(int32 N)
{
    Simulation->Step(N);
    Sync();
}

FString ASimcraftWorld::SaveGame()
{
    return Simulation ? Simulation->Save() : FString();
}

bool ASimcraftWorld::LoadGame(const FString& Save, FString& OutError)
{
    if (!Simulation || !Simulation->LoadSave(Save, OutError)) return false;
    Sync();
    return true;
}

void ASimcraftWorld::Sync()
{
    TSet<int64> Seen;
    for (const FSimcraftEntity& E : Simulation->GetEntities())
    {
        Seen.Add(E.Id);
        FView* View = Live.Find(E.Id);
        if (!View)
        {
            const TSubclassOf<AActor>* Class = KindActors.Find(E.Kind);
            if (!Class || !*Class) continue;
            FActorSpawnParameters Params;
            Params.Owner = this;
            AActor* Actor = GetWorld()->SpawnActor<AActor>(*Class, GetActorTransform(), Params);
            if (!Actor) continue;
            if (Actor->Implements<USimcraftView>()) ISimcraftView::Execute_OnSimcraftSpawn(Actor, E.Id, E.Kind);
            View = &Live.Add(E.Id, FView{Actor, FString()});
        }
        AActor* Actor = View->Actor.Get();
        if (!Actor) continue;
        Actor->SetActorLocation(GetActorLocation() + FVector(E.X * CellSize, -E.Y * CellSize, 0.f));
        if (View->Glyph != E.Glyph)
        {
            View->Glyph = E.Glyph;
            if (Actor->Implements<USimcraftView>()) ISimcraftView::Execute_OnSimcraftState(Actor, E.Glyph);
        }
    }
    for (auto It = Live.CreateIterator(); It; ++It)
    {
        if (Seen.Contains(It.Key())) continue;
        if (AActor* Actor = It.Value().Actor.Get()) Actor->Destroy();
        It.RemoveCurrent();
    }
    const FString Msgs = Simulation->Drain();
    if (Msgs != TEXT("[]")) OnBusMessages.Broadcast(Msgs);
}
