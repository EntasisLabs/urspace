import { Nav } from './components/Nav'
import { Hero } from './components/Hero'
import { Principles } from './components/Principles'
import { Control } from './components/Control'
import { HowItWorks } from './components/HowItWorks'
import { Modes } from './components/Modes'
import { Position } from './components/Position'
import { Install } from './components/Install'
import { Footer } from './components/Footer'

export default function App() {
  return (
    <>
      <Nav />
      <main>
        <Hero />
        <HowItWorks />
        <Control />
        <Principles />
        <Modes />
        <Position />
        <Install />
      </main>
      <Footer />
    </>
  )
}
