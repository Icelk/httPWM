document.getElementById("effectOurLocation").innerText = location.origin

let mainStrength = document.getElementById("strength")

let from = document.getElementById("fromStrength")
let to = document.getElementById("toStrength")
let time = document.getElementById("transitionTime")
let interpolation = document.getElementById("interpolation")
let interpolationExtras = document.getElementById("interpolationExtras")

let day = document.getElementById("weekday")
let dayTime = document.getElementById("dayTime")
let dayOption = document.getElementById("optionTime")

let schedulerList = document.getElementById("schedulerList")
let schedulerKind = document.getElementById("schedulerKind")
let schedulerDate = document.getElementById("schedulerDate")
let schedulerWeekday = document.getElementById("schedulerWeekday")
let schedulerTime = document.getElementById("schedulerTime")
let schedulerName = document.getElementById("schedulerName")
let schedulerDescription = document.getElementById("schedulerDescription")

let effectPreview = document.getElementById("effect-preview")
let effectRemotesList = document.getElementById("effect-remotes")
let effectSelf = document.getElementById("effect-self")
let effectCenter = document.getElementById("effect-center")
let effectRadar = document.getElementById("effect-radar")
let effectSpeed = document.getElementById("effect-speed")
let effectAdd = document.getElementById("effect-remote-add")
let effectRemoteName = document.getElementById("effect-remote-name")
let effectUrl = document.getElementById("effect-remote-url")
let effectSubmit = document.getElementById("effect-submit")

/**
 * @type { {[name: string]: {backlog: Event[], inTimeout: boolean}} }
 */
let throttle_instances = {}

/**
 * Throttles calling `callback` to every `interval` milliseconds.
 * If this is called more than once in the hang period, only the latest callback is called.
 *
 * If it's called once, which calls the callback, and then once again, it waits `interval` before
 * calling `callback` again.
 *
 * @param {string} name
 * @param {number} interval
 * @param {() => Promise<void>} callback
 */
async function throttle(name, interval, callback) {
    if (throttle_instances[name] === undefined) {
        throttle_instances[name] = {
            backlog: [],
            inTimeout: false,
        }
    }

    let instance = throttle_instances[name]

    if (instance.inTimeout) {
        instance.backlog.push(callback)
        return
    }

    await callback()

    instance.inTimeout = true
    setTimeout(async () => {
        instance.inTimeout = false
        let item = instance.backlog.pop()

        if (item !== undefined) {
            instance.backlog.length = 0
            await item()
        }
    }, interval)
}

window.addEventListener("unhandledrejection", (message) => {
    if (message.reason.message === "Failed to fetch") {
        message.preventDefault()
        sendNotification("Server is offline.", notificationError)
    }
})

let currentNotification = null
const notificationInfo = "#2f5faf9f"
const notificationError = "#8c1c2e9f"
const notificationTransform = "translateX(calc(200% + 2em))"
let notificationTimeout = 5000
function sendNotification(message, color) {
    tryClearNotification()

    let element = document.createElement("span")
    element.classList.add("notification")
    element.style.backgroundColor = color
    element.innerHTML = message
    element.addEventListener("click", () => tryClearNotification())

    document.body.appendChild(element)

    setTimeout(() => {
        element.style.transform = "none"
    }, 20)

    let timeout1 = setTimeout(() => {
        currentNotification.element.style.transform = notificationTransform
    }, notificationTimeout)
    let timeout2 = setTimeout(() => {
        tryClearNotification()
    }, notificationTimeout + 500)

    currentNotification = { element: element, timeout1: timeout1, timeout2: timeout2 }
}
function tryClearNotification() {
    if (currentNotification !== null) {
        let element = currentNotification.element

        element.style.transform = notificationTransform
        setTimeout(() => {
            document.body.removeChild(element)
        }, 500)
        clearTimeout(currentNotification.timeout1)
        clearTimeout(currentNotification.timeout2)

        currentNotification = null
    }
}
function responseNotification(response, name, quiet = false) {
    if (response.ok) {
        if (!quiet) sendNotification(`${name} succeeded!`, notificationInfo)
    } else {
        sendNotification(`${name} failed (${response.statusText})`, notificationError)
    }
}

function removeAllChildren(element) {
    while (element.children.length > 0) {
        element.removeChild(element.children[0])
    }
}

async function sendSet(strength) {
    let response = await fetch(`/set-strength?strength=${strength}`)
    responseNotification(response, "Set strength", true)
}
// Day must exist, can be 'mon', 'tue', etc.
// Time can be null or "HH:MM:SS" format.
async function sendDayTime(day, time) {
    let response = await fetch("/set-day-time", {
        method: "POST",
        headers: {
            "content-type": "application/json",
        },
        redirect: "error",
        body: JSON.stringify({ day: day, time: time }),
    })
    responseNotification(response, "Set day time")
    // Takes a bit of time in backend to send message between threads...
    // (it's to damn fast)
    setTimeout(async () => await getAndApplyState(), 50)
}
function getAndSendDayTime() {
    if (dayOption.value === "some") {
        sendDayTime(day.value, dayTime.value)
    } else {
        sendDayTime(day.value, null)
    }
}
function getTransition() {
    return {
        from: Number(from.value),
        to: Number(to.value),
        time: Number(time.value),
        interpolation: interpolation.value,
        extras: [interpolationExtras.value],
    }
}
async function getAndSetTransition(action) {
    let response = await fetch(`/transition?action=${action}`, {
        method: "POST",
        headers: {
            "content-type": "application/json",
        },
        redirect: "error",
        body: JSON.stringify(getTransition()),
    })
    responseNotification(response, `${action} transition`)
}
function checkTransitionExtras() {
    interpolationExtras.style.display = interpolation.value.endsWith("-extra") ? "initial" : "none"
}
function checkDailySchedulerOption() {
    dayTime.style.display = dayOption.value === "some" ? "initial" : "none"
}

async function getAndApplyState() {
    let response = await fetch("/get-state")
    responseNotification(response, "Update state", true)

    let json = await response.json()

    mainStrength.value = json.strength
    for (const day in json.days) {
        const time = json.days[day]
        const element = document.getElementById(day)

        if (element !== null) {
            element.innerHTML = time === null ? `No time set.` : `Time set at ${time}`
        }
    }
}

async function removeScheduler(name) {
    let response = await fetch(`/remove-scheduler?name=${name}`)
    responseNotification(response, "Removed scheduler")
    await overrideSchedulerList()
}

async function overrideSchedulerList() {
    let response = await fetch("/get-schedulers")
    responseNotification(response, "Get schedulers", true)
    let list = await response.json()

    const none = list.length == 0

    if (none) {
        list.push({ name: "N/A", description: "N/A", kind: "none defined", next_occurrence: "N/A" })
    }

    removeAllChildren(schedulerList)

    for (let index = 0; index < list.length; index++) {
        const data = list[index]
        let tr = document.createElement("tr")

        let name = document.createElement("td")
        if (!none) {
            let remove = document.createElement("a")
            remove.innerHTML = "X"
            remove.classList.add("remove-scheduler")
            remove.addEventListener("click", (t) => removeScheduler(t.target.nextSibling.wholeText))
            name.appendChild(remove)
        }
        name.appendChild(document.createTextNode(data.name))
        let description = document.createElement("td")
        description.innerHTML = data.description
        let kind = document.createElement("td")
        kind.innerHTML = data.kind
        let next = document.createElement("td")
        next.innerHTML = data.next_occurrence

        tr.appendChild(name)
        tr.appendChild(description)
        tr.appendChild(kind)
        tr.appendChild(next)
        schedulerList.appendChild(tr)
    }
}

function checkSchedulerAddExtras() {
    let { date, day } = getSchedulerExtras()

    schedulerDate.style.display = date ? "" : "none"
    schedulerWeekday.style.display = day ? "" : "none"
}
function getSchedulerExtras() {
    let kind = schedulerKind.value
    let date = false
    let day = false

    if (kind === "at") {
        date = true
    } else if (kind === "every-week") {
        day = true
    }
    return { date: date, day: day }
}
async function getAndAddScheduler() {
    let name = schedulerName.value
    let description = schedulerDescription.value

    if (name === "" || description === "") {
        sendNotification("Please specify a name and description.", notificationInfo)
        return
    }

    let kind = schedulerKind.value
    let time = schedulerTime.value
    let extras = []
    let { date: send_date, day: send_day } = getSchedulerExtras()
    if (send_date) {
        extras.push(schedulerDate.value)
    }
    if (send_day) {
        extras.push(schedulerWeekday.value)
    }

    const body = {
        kind: kind,
        time: time,
        name: name,
        description: description,
        extras: extras,
        transition: getTransition(),
    }

    console.log(body)

    let response = await fetch("/add-scheduler", {
        method: "POST",
        headers: {
            "content-type": "application/json",
        },
        redirect: "error",
        body: JSON.stringify(body),
    })
    responseNotification(response, "Added scheduler")
    await overrideSchedulerList()
}

async function load() {
    let state = getAndApplyState()
    let schedulers = overrideSchedulerList()

    let h2s = document.getElementsByTagName("h2")
    let loadedData = JSON.parse(localStorage.getItem("collapsed"))
    if (loadedData === null) {
        loadedData = {}
        localStorage.setItem("collapsed", JSON.stringify({}))
    }
    for (const header of h2s) {
        const target = header.getAttribute("toggle")
        if (target !== null) {
            const toggled = document.getElementById(target)
            header.addEventListener("click", () => {
                let data = JSON.parse(localStorage.getItem("collapsed"))

                if (toggled.style.maxHeight === "0px") {
                    // extra 2em if you make the screen smaller, max height should have headroom
                    toggled.style.maxHeight = `calc(${toggled.scrollHeight}px + 2em)`

                    header.classList.add("expanded")
                    data[target] = "expanded"
                } else {
                    toggled.style.maxHeight = "0px"

                    header.classList.remove("expanded")
                    delete data[target]
                }
                localStorage.setItem("collapsed", JSON.stringify(data))
            })
            header.classList.add("collapse-host")

            if (loadedData !== null && loadedData[target] === "expanded") {
                // so the UI has time to draw, so we can get scrollHeight
                toggled.style.maxHeight = `100vh`
                setTimeout(() => {
                    toggled.style.maxHeight = `calc(${toggled.scrollHeight}px + 2em)`
                }, 0)

                header.classList.add("expanded")
            } else {
                toggled.style.maxHeight = "0px"
            }
            toggled.classList.add("collapsible")
        }
    }

    await Promise.all([state, schedulers])
}

window.addEventListener("resize", () => {
    let h2s = document.getElementsByTagName("h2")
    for (const header of h2s) {
        const target = header.getAttribute("toggle")
        if (target !== null) {
            const toggled = document.getElementById(target)
            if (header.classList.contains("expanded")) {
                // so the UI has time to draw, so we can get scrollHeight
                toggled.style.maxHeight = `100vh`
                setTimeout(() => {
                    toggled.style.maxHeight = `calc(${toggled.scrollHeight}px + 2em)`
                }, 0)

                header.classList.add("expanded")
            }
        }
    }
})

// effect remotes
let effectRemotes = {}
let effectActiveDrag = null
let effectDur = 5
function saveEffectPreviewPositions(remove = false) {
    // get all points' positions
    let positions = {}
    let ch = effectPreview.lastElementChild
    while (ch.id !== "effect-self") {
        let c = ch
        positions[c.getAttribute("name")] = [c.style.left, c.style.top]
        // remove all points in preview
        if (remove) {
            c.remove()
            ch = effectPreview.lastElementChild
        } else {
            ch = c.previousElementSibling
        }
    }
    let filtered_positions = {}
    Object.keys(effectRemotes).forEach((remote) => (filtered_positions[remote] = positions[remote]))
    filtered_positions["self"] = [effectSelf.style.left, effectSelf.style.top]
    filtered_positions["center"] = [effectCenter.style.left, effectCenter.style.top]
    // localStorage set
    localStorage.setItem("effect-positions", JSON.stringify(filtered_positions))
    return filtered_positions
}
function updateEffectPreview(startup = false) {
    let filtered_positions = !startup
        ? saveEffectPreviewPositions(true)
        : JSON.parse(localStorage.getItem("effect-positions") ?? "{}") ?? {}

    localStorage.setItem("effect-remotes", JSON.stringify(effectRemotes))

    const setCoordinates = (node, remote, node2 = undefined) => {
        let x, y
        if (filtered_positions[remote] !== undefined) {
            x = filtered_positions[remote][0]
            y = filtered_positions[remote][1]
        } else {
            x = `${Math.random() * 100}%`
            y = `${Math.random() * 100}%`
        }
        node.style.left = x
        node.style.top = y
        if (node2 !== undefined) {
            node2.style.left = x
            node2.style.top = y
        }
    }

    while (effectRemotesList.children.length > 1) {
        effectRemotesList.lastElementChild.remove()
    }
    // add all points (if no previous position, place at random position)
    Object.entries(effectRemotes).forEach(([remote, remote_url]) => {
        let randomColor = `hsl(${Math.round(Math.random() * 360)}deg 80% 50%)`

        let node = document.createElement("span")
        node.setAttribute("name", remote)
        node.title = remote
        node.style.backgroundColor = randomColor
        setCoordinates(node, remote)
        node.addEventListener("mousedown", () => (effectActiveDrag = node))
        effectPreview.appendChild(node)

        let tr = document.createElement("tr")
        let name = document.createElement("td")
        let color = document.createElement("span")
        let url = document.createElement("td")
        let status = document.createElement("td")

        name.innerText = remote
        color.style.backgroundColor = randomColor
        color.classList.add("effect-remote-color")
        name.insertBefore(color, name.firstChild)
        url.innerText = remote_url
        status.innerText = "Pinging..."
        fetch(`${remote_url}/get-state`)
            .then((response) => {
                if (response.ok) {
                    status.innerText = "Online"
                } else {
                    status.innerText = "Invalid endpoint"
                }
            })
            .catch((_) => {
                status.innerText = "Offline"
            })

        tr.appendChild(name)
        tr.appendChild(url)
        tr.appendChild(status)
        effectRemotesList.appendChild(tr)
    })
    if (startup) {
        setCoordinates(effectSelf, "self")
        setCoordinates(effectCenter, "center", effectRadar)
        effectCenter.addEventListener("mousedown", () => {
            effectActiveDrag = effectCenter
        })
        effectSelf.addEventListener("mousedown", () => {
            effectActiveDrag = effectSelf
        })
    }
}
function clamp(x, min, max) {
    return Math.min(Math.max(x, min), max)
}
window.addEventListener("mouseup", () => {
    effectActiveDrag = null
    saveEffectPreviewPositions()
})
window.addEventListener("mousemove", (e) => {
    if (effectActiveDrag !== null) {
        let x = e.x
        let y = e.y
        let rect = effectPreview.getBoundingClientRect()
        let localX = (x - rect.left) / rect.width
        let localY = (y - rect.top) / rect.height
        localX = clamp(localX, 0, 1)
        localY = clamp(localY, 0, 1)
        effectActiveDrag.style.top = `${localY * 100}%`
        effectActiveDrag.style.left = `${localX * 100}%`
        // update radar line
        if (effectActiveDrag === effectCenter) {
            effectRadar.style.top = `${localY * 100}%`
            effectRadar.style.left = `${localX * 100}%`
        }
    }
})
effectSpeed.addEventListener("change", () => {
    let v = +effectSpeed.value
    if (isFinite(v) && v >= 0.2) {
        effectDur = v
        effectRadar.style.animationDuration = `${v}s`
    }
})
effectAdd.addEventListener("click", () => {
    if (effectRemoteName.value && effectUrl.value) {
        effectRemotes[effectRemoteName.value] = effectUrl.value
        updateEffectPreview()
    } else if (effectRemoteName.value) {
        delete effectRemotes[effectRemoteName.value]
        updateEffectPreview()
    }
})
effectSubmit.addEventListener("click", () => {
    let centerPos = [+effectCenter.style.left.split("%")[0] / 100, +effectCenter.style.top.split("%")[0] / 100]
    let speed = effectDur

    let remotes = []
    let ch = effectPreview.lastElementChild
    while (ch.id !== "effect-center") {
        let c = ch
        let pos = [+c.style.left.split("%")[0] / 100, +c.style.top.split("%")[0] / 100]

        let v = Math.atan2(pos[1] - centerPos[1], pos[0] - centerPos[0])
        let turns = v / (Math.PI * 2)
        if (turns < 0) {
            turns += 1
        }
        let offset = turns * speed

        remotes.push([effectRemotes[c.getAttribute("name")] ?? location.href, offset])
        ch = c.previousElementSibling
    }
    remotes.forEach(([url, delay]) => {
        console.log(url, `${delay * 1000}ms`)
    })
})

load()
checkTransitionExtras()
checkDailySchedulerOption()
checkSchedulerAddExtras()
effectRemotes = JSON.parse(localStorage.getItem("effect-remotes") ?? "{}") ?? {}
updateEffectPreview(true)
