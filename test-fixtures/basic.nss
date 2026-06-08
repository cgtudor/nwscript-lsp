// Basic NWScript test file for parser validation.
#include "nwnx_player"

const int MY_CONSTANT = 42;
const string MY_STRING = "hello";

struct MyStruct
{
    int nValue;
    string sName;
    float fWeight;
};

// Forward declaration
void DoSomething(object oPC, int nParam = 0);

// Function with body
void main()
{
    object oPC = GetFirstPC();
    int nLevel = GetHitDice(oPC);

    if (nLevel > 10)
    {
        SendMessageToPC(oPC, "High level!");
    }
    else
    {
        SendMessageToPC(oPC, "Keep going.");
    }

    struct MyStruct sData;
    sData.nValue = nLevel;
    sData.sName = GetName(oPC);

    int i;
    for (i = 0; i < 10; i++)
    {
        // Loop body
        int nTemp = i * 2;
    }

    switch (nLevel)
    {
        case 1:
            break;
        case 5:
            DoSomething(oPC, 1);
            break;
        default:
            DoSomething(oPC);
            break;
    }

    return;
}

void DoSomething(object oPC, int nParam = 0)
{
    string sMsg = "Param: " + IntToString(nParam);
    SendMessageToPC(oPC, sMsg);
}
