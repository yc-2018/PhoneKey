#define INITGUID
#include <windows.h>
#include <credentialprovider.h>

// The SDK ships these IIDs in a separate import library that MinGW lacks.
// Define only the ones this provider actually queries.
// {63913a93-40c1-481a-818d-4072ff8c70cc}
extern "C" const IID IID_ICredentialProviderCredential =
    {0x63913a93, 0x40c1, 0x481a, {0x81, 0x8d, 0x40, 0x72, 0xff, 0x8c, 0x70, 0xcc}};

// {d27c3481-5a1c-45b2-8aaa-c20ebbe8229e}
extern "C" const IID IID_ICredentialProvider =
    {0xd27c3481, 0x5a1c, 0x45b2, {0x8a, 0xaa, 0xc2, 0x0e, 0xbb, 0xe8, 0x22, 0x9e}};
