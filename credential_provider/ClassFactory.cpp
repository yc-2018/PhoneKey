#include <windows.h>
#include <new>
#include "ClassFactory.h"
#include "UnlockProvider.h"

extern LONG g_cRef;

CClassFactory::CClassFactory() : _cRef(1)
{
    InterlockedIncrement(&g_cRef);
}

CClassFactory::~CClassFactory()
{
    InterlockedDecrement(&g_cRef);
}

HRESULT CClassFactory::QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv)
{
    if (!ppv) return E_INVALIDARG;

    if (riid == IID_IClassFactory || riid == IID_IUnknown) {
        *ppv = static_cast<IClassFactory*>(this);
        AddRef();
        return S_OK;
    }

    *ppv = nullptr;
    return E_NOINTERFACE;
}

ULONG CClassFactory::AddRef()
{
    return InterlockedIncrement(&_cRef);
}

ULONG CClassFactory::Release()
{
    LONG cRef = InterlockedDecrement(&_cRef);
    if (cRef == 0) {
        delete this;
    }
    return cRef;
}

HRESULT CClassFactory::CreateInstance(
    _In_opt_ IUnknown* pUnkOuter,
    _In_ REFIID riid,
    _Outptr_ void** ppv)
{
    if (!ppv) return E_INVALIDARG;
    *ppv = nullptr;

    if (pUnkOuter) return CLASS_E_NOAGGREGATION;

    CUnlockProvider* pProvider = new(std::nothrow) CUnlockProvider();
    if (!pProvider) return E_OUTOFMEMORY;

    HRESULT hr = pProvider->QueryInterface(riid, ppv);
    pProvider->Release();
    return hr;
}

HRESULT CClassFactory::LockServer(_In_ BOOL bLock)
{
    if (bLock) {
        InterlockedIncrement(&g_cRef);
    } else {
        InterlockedDecrement(&g_cRef);
    }
    return S_OK;
}
