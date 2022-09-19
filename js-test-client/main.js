require('./tracing');
const opentelemetry = require("@opentelemetry/api");
const tracer = opentelemetry.trace.getTracer('main');

const mainWork = () => {
    tracer.startActiveSpan('main', parentSpan => {
        for (let i = 0; i < 3; i += 1) {
            doWork(i);
        }
        doSiblingWork();
        // Be sure to end the parent span!
        parentSpan.end();
    });
}

function doWork(i) {
    console.log(`doing work for ${i}`);

    tracer.startActiveSpan(`doWork`, span => {
        compute();
        // Make sure to end this child span! If you don't it will continue to track work beyond 'doWork'!
        span.end();
    });
}

const doSiblingWork = () => {
    console.log(`doing sibling work`);
    const span1 = tracer.startSpan('siblingwork');
    compute();

    const span2 = tracer.startSpan('siblingwork');
    compute();

    const span3 = tracer.startSpan('siblingwork');
    compute();

    span1.end();
    span2.end();
    span3.end();
};

function compute() {
    // simulate some random work.
    let n = 2;
    for (let i = 0; i <= Math.floor(Math.random() * 400000000); i += 1) {
        n *= 2;
    }
    console.log(n);
}

mainWork();
