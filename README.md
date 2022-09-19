# modality-otel-receiver

## working notes
Mapping Resources and Spans to Timelines and Events
- Resource attributes always become timeline attributes
  - what if resource is null?
- how to choose a timeline name:
  - ${resource.namespace}.${resource.name}.${resource.instance.id}
  - customizable as list of keys, and separator character

What about value mapping?
Attr vals can be:
- string
- bool
- int64
- double
- bytes
- array of the above above
- str->val dict of the above

for arrays / dicts, flatten with expanded keys.

What about Baggage?
- this doesn't exist in otlp

What about 'scope'?
- this should probably go on the timeline, under '.scope'.

What about ordering?
- We need to queue of spans until all parent links are resolved. If we try to shut down before that happens, flush anything pending and leave the parent link dangling
- or do we? If each span gets its own timeline, we don't actually care. 
- But if we buffer, we can opportunisticly flatten to fewer timelines based on:
  - spans with the same 'resource'
  - span concurrency / overlapping
    - We could also flatten more aggressively, even with overlapping stuff
    - this could also be configurable
  - configurable time for buffering

What about span links?
- probably just attr conversion?
    
Timeline allocation styles:
- timeline per span
- timeline per scope-resource-trace, flattened
- timeline per scope-resource-trace, partially flattened
  - spans which appear to be concurrent get their own timelines

How do spans look?
- start/end events
- events are named "<span name>" and "<span name>_end"

The actual otel data model:
   Trace
    Resource
      Scope
        Span
 
